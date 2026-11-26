//! `mcp serve`: expose the existing tool registry over MCP stdio.
//!
//! Like the original server, this is a local tool execution entry point. The
//! MCP host owns user confirmation; this path does not launch a TUI or query a
//! model. Tool schemas, input validation, execution and raw output all belong
//! to the same implementations used by the normal assistant.

use crate::services::tools::tool_execution::{find_tool_call, validate_tool_input_against_schema};
use crate::tool::{
    AbortController, ToolPermissionContext, ToolPromptOptions, ToolUseContext, ValidationResult,
};
use crate::types::message::ToolResultStatus;
use crate::types::permissions::PermissionMode;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer, RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::transport::{Transport, TransportAdapterIdentity};
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::Notify;

pub fn print_help() {
    println!(
        "Usage: cometix mcp serve [options]\n\nStart the Claude Code MCP server\n\nOptions:\n  -d, --debug  Enable debug mode\n  -h, --help   Display help for command\n  --verbose    Override the verbose setting"
    );
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    for arg in args {
        if !matches!(arg.as_str(), "-d" | "--debug" | "--verbose") {
            anyhow::bail!("error: unexpected argument '{arg}' for 'mcp serve'");
        }
    }
    crate::utils::tls_provider::install_crypto_provider();
    // Bash and several other tools execute synchronously. A current-thread
    // runtime would let one such call starve the MCP message loop, so keep a
    // second worker available for protocol traffic and cancellation. The
    // resource wrapper pins the whole process to one logical CPU.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    // This runtime is block_on-driven for the whole MCP session, so it is the
    // process-lifetime runtime for detached work (plugin loading and friends),
    // matching the publish step in main.rs and cli/print.rs.
    crate::utils::process_runtime::set_process_runtime_handle(runtime.handle().clone());
    runtime.block_on(async {
        let mut server = McpToolServer::new();
        server.context.debug = args.iter().any(|arg| arg == "-d" || arg == "--debug");
        server.context.verbose = args.iter().any(|arg| arg == "--verbose");
        let abort = server.context.abort_controller.clone();
        let eof = Arc::new(Notify::new());
        let (stdin, stdout) = rmcp::transport::stdio();
        let transport = EofAwareTransport {
            inner: AsyncRwTransport::new_server(stdin, stdout),
            eof: Arc::clone(&eof),
        };
        let service = server
            .serve::<EofAwareTransport, std::io::Error, TransportAdapterIdentity>(transport)
            .await?;
        let cancel_on_signal = service.cancellation_token();
        let cancel_on_eof = service.cancellation_token();
        let mut finished = tokio::spawn(service.waiting());
        tokio::select! {
            result = &mut finished => { result??; }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                abort.abort();
                cancel_on_signal.cancel();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut finished).await;
            }
            _ = eof.notified() => {
                abort.abort();
                cancel_on_eof.cancel();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut finished).await;
            }
        }
        abort.abort();
        anyhow::Ok(())
    })
}

struct McpToolServer {
    context: ToolUseContext,
    // Preserve Read -> Edit ordering and avoid concurrent writes to the same
    // file. This also bounds tool execution on a small development machine.
    execution: Arc<Mutex<()>>,
}

impl McpToolServer {
    fn new() -> Self {
        let mut context = ToolUseContext::with_permission_context(ToolPermissionContext {
            mode: PermissionMode::BypassPermissions,
            ..ToolPermissionContext::default()
        });
        context.is_non_interactive_session = true;
        context.tools.retain(|tool| {
            // These tools require an interactive question/plan dialog, which a
            // stdio MCP host cannot render. Do not advertise unavailable UI.
            !matches!(
                tool.name.as_str(),
                "AskUserQuestion" | "EnterPlanMode" | "ExitPlanMode"
            ) && find_tool_call(&tool.name).is_some()
        });
        Self {
            context,
            execution: Arc::new(Mutex::new(())),
        }
    }

    fn tools(&self) -> Vec<Tool> {
        let options = ToolPromptOptions {
            tool_permission_context: &self.context.tool_permission_context,
            tools: &self.context.tools,
            agents: &[],
            allowed_agent_types: None,
        };
        self.context
            .tools
            .iter()
            .filter_map(|schema| {
                let call = find_tool_call(&schema.name)?;
                let input = schema.input_schema.as_object()?.clone();
                Some(Tool::new(
                    schema.name.clone(),
                    call.prompt(schema, &options),
                    input,
                ))
            })
            .collect()
    }

    async fn execute(
        &self,
        request: CallToolRequestParams,
        rpc: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.as_ref();
        let schema = self
            .context
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .ok_or_else(|| ErrorData::internal_error(format!("Tool {name} not found"), None))?;
        let call = find_tool_call(name)
            .ok_or_else(|| ErrorData::internal_error(format!("Tool {name} not found"), None))?;
        let raw = Value::Object(request.arguments.unwrap_or_default());
        validate_tool_input_against_schema(name, &raw, &schema.input_schema)
            .map_err(|error| ErrorData::invalid_params(error, None))?;
        let mut context = self.context.clone();
        context.abort_controller = AbortController::child_of(self.context.abort_controller.clone());
        context.tool_use_id = Some(format!("mcp_{}", uuid::Uuid::new_v4()));
        let input = call.normalize_input_with_context(&raw, &context);
        match call.validate_input(&input, &context) {
            ValidationResult::Ok => {}
            ValidationResult::Error { message, .. } | ValidationResult::Fatal { message } => {
                return Err(ErrorData::internal_error(message, None));
            }
        }
        let id = context
            .tool_use_id
            .clone()
            .unwrap_or_else(|| "mcp".to_string());
        let permission_request =
            crate::utils::permissions::permissions::mock_permission_request_with_input(
                id.as_str(),
                id.as_str(),
                name,
                String::new(),
                input.clone(),
                PermissionMode::BypassPermissions,
            );
        let abort_controller = context.abort_controller.clone();
        let execution_lock = Arc::clone(&self.execution);
        let tool_abort_controller = abort_controller.clone();
        let execution = tokio::spawn(async move {
            let _execution = execution_lock.lock().await;
            let _abort_on_drop = AbortOnDrop(tool_abort_controller);
            call.call(&input, &permission_request, &context, None, None, None)
                .await
        });
        tokio::pin!(execution);
        let output = tokio::select! {
            output = &mut execution => output
                .map_err(|error| ErrorData::internal_error(format!("Tool task failed: {error}"), None))?,
            _ = rpc.ct.cancelled() => {
                abort_controller.abort();
                // Give process-backed tools time to reap the child they own.
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), &mut execution).await;
                return Err(ErrorData::internal_error("Tool call cancelled", None));
            }
        };
        let (text, status) = call.map_tool_result_to_tool_result_block_param(&output.data, &id);
        if status != ToolResultStatus::Success {
            return Err(ErrorData::internal_error(text, None));
        }
        let text = match call.tool_use_result(&output.data) {
            Some(raw) => serde_json::to_string(&raw)
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?,
            None => text,
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
    }
}

/// Observe stdio EOF so shutdown can cancel an in-flight tool immediately
/// instead of waiting for the transport drain timeout.
struct EofAwareTransport {
    inner: AsyncRwTransport<RoleServer, tokio::io::Stdin, tokio::io::Stdout>,
    eof: Arc<Notify>,
}

impl Transport<RoleServer> for EofAwareTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    fn receive(
        &mut self,
    ) -> impl std::future::Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
        async move {
            let message = self.inner.receive().await;
            if message.is_none() {
                self.eof.notify_one();
            }
            message
        }
    }

    fn close(&mut self) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

struct AbortOnDrop(AbortController);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl ServerHandler for McpToolServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("cometix", crate::constants::product::VERSION),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools(),
            ..ListToolsResult::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.execute(request, context).await
    }
}
