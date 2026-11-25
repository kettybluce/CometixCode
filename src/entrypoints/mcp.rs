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
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::sync::Mutex;

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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut server = McpToolServer::new();
        server.context.debug = args.iter().any(|arg| arg == "-d" || arg == "--debug");
        server.context.verbose = args.iter().any(|arg| arg == "--verbose");
        let abort = server.context.abort_controller.clone();
        let service = server.serve(rmcp::transport::stdio()).await?;
        let cancellation = service.cancellation_token();
        let finished = service.waiting();
        tokio::pin!(finished);
        tokio::select! {
            result = &mut finished => { result?; }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                abort.abort();
                cancellation.cancel();
                finished.await?;
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
    execution: Mutex<()>,
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
            execution: Mutex::new(()),
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
        let _execution = tokio::select! {
            guard = self.execution.lock() => guard,
            _ = rpc.ct.cancelled() => return Err(ErrorData::internal_error("Tool call cancelled", None)),
        };
        let mut context = self.context.clone();
        context.abort_controller = AbortController::child_of(self.context.abort_controller.clone());
        context.tool_use_id = Some(format!("mcp_{}", uuid::Uuid::new_v4()));
        let _abort_on_drop = AbortOnDrop(context.abort_controller.clone());
        let input = call.normalize_input_with_context(&raw, &context);
        match call.validate_input(&input, &context) {
            ValidationResult::Ok => {}
            ValidationResult::Error { message, .. } | ValidationResult::Fatal { message } => {
                return Err(ErrorData::internal_error(message, None));
            }
        }
        let id = context.tool_use_id.as_deref().unwrap_or("mcp");
        let permission_request =
            crate::utils::permissions::permissions::mock_permission_request_with_input(
                id,
                id,
                name,
                String::new(),
                input.clone(),
                PermissionMode::BypassPermissions,
            );
        let execution = call.call(&input, &permission_request, &context, None, None, None);
        tokio::pin!(execution);
        let output = tokio::select! {
            output = &mut execution => output,
            _ = rpc.ct.cancelled() => {
                context.abort_controller.abort();
                // Give process-backed tools time to reap the child they own.
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), &mut execution).await;
                return Err(ErrorData::internal_error("Tool call cancelled", None));
            }
        };
        let (text, status) = call.map_tool_result_to_tool_result_block_param(&output.data, id);
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
