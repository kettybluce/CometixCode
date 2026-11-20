//! `/mcp` command seam.
//! Maps to official `src/commands/mcp/mcp.tsx`.
//!
//! Item-2 state delegates UI to `components/mcp/MCPSettings.tsx` and
//! `MCPReconnect.tsx` equivalents. Toggle/reconnect dispatch stays at the
//! command/component seam and forwards runtime work to `services/mcp/*`, matching
//! official `useMcpToggleEnabled()` / `useMcpReconnect()` boundaries.

use crate::components::mcp::MCPSettings;
use crate::components::mcp::mcp_reconnect::MCPReconnect;
use crate::services::mcp::types::{McpServerConnection, McpServerConnectionType};
use crate::utils::process_env::JsTruthy;
use crate::utils::settings::constants::SettingSource;
use crate::utils::settings::get_settings_file_path_for_source;
use iocraft::prelude::*;
use serde_json::Value;
use std::io::{IsTerminal, Read};

#[cfg(test)]
pub const NO_MCP_SERVERS_CONFIGURED: &str =
    crate::components::mcp::mcp_settings::NO_MCP_SERVERS_CONFIGURED;

/// Maps to: CC `commands/mcp/xaaIdpCommand.ts#registerMcpXaaIdpCommand`.
///
/// This is intentionally a thin CLI surface: it parses flags, writes the
/// non-secret `settings.xaaIdp` config, and dispatches secret/cache/login work
/// to `services/mcp/xaa.rs`.
pub fn maybe_run_mcp_xaa_cli(argv: &[String], rt: &tokio::runtime::Runtime) -> Option<i32> {
    let mcp_index = argv.iter().position(|arg| arg == "mcp")?;
    if argv.get(mcp_index + 1).map(String::as_str) != Some("xaa") {
        return None;
    }
    if !crate::services::mcp::xaa_idp_login::is_xaa_enabled() {
        eprintln!("error: unknown command 'xaa'");
        return Some(1);
    }
    let args = &argv[mcp_index + 2..];
    Some(match run_mcp_xaa_cli(args, rt) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

/// Handle the non-interactive MCP configuration commands that must run before
/// the retained TUI starts. This mirrors Claude Code's `mcp add-json` command;
/// the `-` payload is read from stdin so secrets never appear in argv/history.
pub fn maybe_run_mcp_cli(argv: &[String]) -> Option<i32> {
    let mcp_index = argv.iter().position(|arg| arg == "mcp")?;
    let subcommand = argv.get(mcp_index + 1).map(String::as_str)?;
    let args = &argv[mcp_index + 2..];
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        match subcommand {
            "add-json" => print_mcp_add_json_help(),
            "get" => print_mcp_get_help(),
            "list" => print_mcp_list_help(),
            _ => return None,
        }
        return Some(0);
    }

    match subcommand {
        "add-json" => Some(match run_mcp_add_json_cli(args) {
            Ok((name, scope)) => {
                println!("Added MCP server '{name}' to {scope} scope.");
                0
            }
            Err(error) => {
                eprintln!("Error: {error}");
                1
            }
        }),
        "get" => Some(match run_mcp_get_cli(args) {
            Ok(()) => 0,
            Err(error) => {
                if error.to_string().starts_with("No MCP server named ") {
                    eprintln!("{error}");
                } else {
                    eprintln!("Error: {error}");
                }
                1
            }
        }),
        "list" => Some(match run_mcp_list_cli(args) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("Error: {error}");
                1
            }
        }),
        _ => None,
    }
}

const MCP_ADD_JSON_MAX_STDIN_BYTES: usize = 1024 * 1024;

fn print_mcp_add_json_help() {
    println!(
        "Usage: cometix mcp add-json [options] <name> <json|->\n\nOptions:\n  -s, --scope <scope>  local (default), user, or project\n      --help           Show this help\n\nUse '-' as the JSON argument to read the server configuration from stdin."
    );
}

fn print_mcp_get_help() {
    println!(
        "Usage: cometix mcp get [options] <name>\n\nGet details about an MCP server. Unapproved .mcp.json servers are shown as pending approval and are not connected to; approved servers are health-checked unless disabled for this project.\n\nOptions:\n  -h, --help  Display help for command"
    );
}

fn print_mcp_list_help() {
    println!("Usage: cometix mcp list\n\nList configured MCP servers and check approved servers.");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum McpListState {
    Connected,
    Failed,
    NeedsAuth,
    Pending,
    PendingApproval,
    Disabled,
}

impl McpListState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Failed => "failed",
            Self::NeedsAuth => "needs authentication",
            Self::Pending => "pending",
            Self::PendingApproval => "pending approval",
            Self::Disabled => "disabled",
        }
    }

    fn as_get_status(self) -> &'static str {
        match self {
            Self::Connected => "✔ Connected",
            Self::Failed => "✘ Failed to connect",
            Self::NeedsAuth => "! Needs authentication",
            Self::PendingApproval => "⏸ Pending approval (run `claude` to approve)",
            Self::Pending | Self::Disabled => "- Not configured",
        }
    }
}

struct McpListRow {
    name: String,
    config: crate::services::mcp::types::ScopedMcpServerConfig,
    state: Option<McpListState>,
}

fn project_mcp_list_state(name: &str) -> Option<McpListState> {
    let status = crate::services::mcp::utils::get_project_mcp_server_status_from_settings(
        name,
        &crate::utils::settings::get_initial_settings(),
        crate::utils::settings::is_setting_source_enabled(
            crate::utils::settings::SettingSource::Project,
        ),
        crate::utils::settings::has_skip_dangerous_mode_permission_prompt(),
        false,
    );
    match status {
        crate::services::mcp::utils::ProjectMcpServerStatus::Pending => {
            Some(McpListState::PendingApproval)
        }
        crate::services::mcp::utils::ProjectMcpServerStatus::Rejected => {
            Some(McpListState::Disabled)
        }
        crate::services::mcp::utils::ProjectMcpServerStatus::Approved => None,
    }
}

fn mcp_list_rows(runtime: &tokio::runtime::Runtime) -> anyhow::Result<Vec<McpListRow>> {
    use crate::services::mcp::config::{get_all_mcp_configs, get_mcp_configs_by_scope_readonly};
    use crate::services::mcp::types::ConfigScope;

    let global_config = crate::utils::config::load_global_config();
    let project_config = crate::utils::config::get_current_project_config();
    let configured = runtime.block_on(get_all_mcp_configs(&global_config, &project_config))?;

    let mut rows = configured
        .servers
        .into_iter()
        .map(|(name, config)| McpListRow {
            state: if config.scope == ConfigScope::Project {
                project_mcp_list_state(&name)
            } else {
                crate::services::mcp::config::is_mcp_server_disabled_in_project_config(
                    &name,
                    &project_config,
                )
                .then_some(McpListState::Disabled)
            },
            name,
            config,
        })
        .collect::<Vec<_>>();
    let existing = rows
        .iter()
        .map(|row| row.name.clone())
        .collect::<std::collections::HashSet<_>>();

    if !crate::services::mcp::config::does_enterprise_mcp_config_exist_readonly() {
        let project_servers = get_mcp_configs_by_scope_readonly(
            ConfigScope::Project,
            &global_config,
            &project_config,
        );
        for (name, config) in project_servers.servers {
            if existing.contains(&name) {
                continue;
            }
            let state = project_mcp_list_state(&name);
            rows.push(McpListRow {
                name,
                config,
                state,
            });
        }

        // Configured servers omitted from the active aggregate are either
        // disabled by project settings or hidden by policy. Retain explicitly
        // disabled user/local entries in the list without trying to connect.
        for scope in [ConfigScope::User, ConfigScope::Project, ConfigScope::Local] {
            let scoped = get_mcp_configs_by_scope_readonly(scope, &global_config, &project_config);
            for (name, config) in scoped.servers {
                if existing.contains(&name)
                    || rows.iter().any(|row| row.name == name)
                    || !crate::services::mcp::config::is_mcp_server_disabled_in_project_config(
                        &name,
                        &project_config,
                    )
                {
                    continue;
                }
                rows.push(McpListRow {
                    name,
                    config,
                    state: Some(McpListState::Disabled),
                });
            }
        }
    }

    Ok(rows)
}

fn mcp_scope_label(scope: crate::services::mcp::types::ConfigScope) -> &'static str {
    use crate::services::mcp::types::ConfigScope;

    match scope {
        ConfigScope::Local => "Local config (private to you in this project)",
        ConfigScope::Project => "Project config (shared via .mcp.json)",
        ConfigScope::User => "User config (available in all your projects)",
        ConfigScope::Dynamic => "Dynamic config (from command line)",
        ConfigScope::Enterprise => "Enterprise config (managed by your organization)",
        ConfigScope::Managed => "Managed settings (provided by your organization)",
        ConfigScope::ClaudeAi => "claude.ai config",
    }
}

fn redact_mcp_issue(
    error: &str,
    config: &crate::services::mcp::types::ScopedMcpServerConfig,
) -> String {
    let error = config.url.as_deref().map_or_else(
        || error.to_string(),
        |url| error.replace(url, "<redacted URL>"),
    );
    static URL_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?i)https?://[^\s\])\}"'<>]+"#).expect("valid URL matcher")
    });
    static AUTH_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?i)\b(authorization|proxy-authorization|x-api-key|api[_-]?key|access[_-]?token|token|secret|password)\s*[:=]\s*[^\s,;)]+"#)
            .expect("valid auth matcher")
    });
    let redacted = URL_PATTERN.replace_all(&error, |capture: &regex::Captures<'_>| {
        let raw = &capture[0];
        let Ok(mut url) = url::Url::parse(raw) else {
            return "<redacted URL>".to_string();
        };
        let _ = url.set_username("");
        let _ = url.set_password(None);
        let has_path = url.path() != "/";
        url.set_path(if has_path { "/[redacted]" } else { "/" });
        if let Some(query) = url.query().map(str::to_owned) {
            let redacted_query = query
                .split('&')
                .map(|part| match part.split_once('=') {
                    Some((key, _)) => format!("{key}=REDACTED"),
                    None => format!("{part}=REDACTED"),
                })
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&redacted_query));
        }
        url.set_fragment(None);
        url.to_string()
    });
    let bearer_pattern =
        regex::Regex::new(r"(?i)\bBearer\s+[^\s,;)]+").expect("valid bearer matcher");
    let mut redacted = bearer_pattern
        .replace_all(&redacted, "Bearer REDACTED")
        .into_owned();
    redacted = AUTH_PATTERN
        .replace_all(&redacted, "$1=REDACTED")
        .into_owned();
    for value in config
        .headers
        .values()
        .chain(config.env.values())
        .chain(config.auth_token.iter())
    {
        if !value.is_empty() {
            redacted = redacted.replace(value, "REDACTED");
        }
    }
    redacted
}

fn mcp_scope_removal_path(scope: crate::services::mcp::types::ConfigScope) -> Option<String> {
    use crate::services::mcp::types::ConfigScope;

    match scope {
        ConfigScope::Enterprise => Some(
            crate::services::mcp::utils::describe_mcp_config_file_path(scope),
        ),
        ConfigScope::Local | ConfigScope::Project | ConfigScope::User => None,
        ConfigScope::Dynamic | ConfigScope::Managed | ConfigScope::ClaudeAi => None,
    }
}

fn mcp_get_preflight_state(
    name: &str,
    config: &crate::services::mcp::types::ScopedMcpServerConfig,
    project_config: &crate::utils::config::ProjectConfig,
) -> Option<McpListState> {
    if config.scope == crate::services::mcp::types::ConfigScope::Project {
        return project_mcp_list_state(name);
    }
    crate::services::mcp::config::is_mcp_server_disabled_in_project_config(name, project_config)
        .then_some(McpListState::Disabled)
}

fn mcp_get_connection(
    runtime: &tokio::runtime::Runtime,
    name: &str,
    config: &crate::services::mcp::types::ScopedMcpServerConfig,
    project_config: &crate::utils::config::ProjectConfig,
) -> (McpListState, Option<String>) {
    if let Some(state) = mcp_get_preflight_state(name, config, project_config) {
        return (state, None);
    }

    let connection = runtime.block_on(crate::services::mcp::client::connect_to_server(
        name, config,
    ));
    let (state, issue) = match &connection {
        crate::services::mcp::types::McpServerConnection::Connected(_) => {
            (McpListState::Connected, None)
        }
        crate::services::mcp::types::McpServerConnection::Failed(failed) => (
            McpListState::Failed,
            failed
                .error
                .as_deref()
                .map(|error| redact_mcp_issue(error, config)),
        ),
        crate::services::mcp::types::McpServerConnection::NeedsAuth(_) => {
            (McpListState::NeedsAuth, None)
        }
        crate::services::mcp::types::McpServerConnection::Pending(_) => {
            (McpListState::Pending, None)
        }
        crate::services::mcp::types::McpServerConnection::Disabled(_) => {
            (McpListState::Disabled, None)
        }
    };
    if let crate::services::mcp::types::McpServerConnection::Connected(connected) = &connection {
        crate::services::mcp::client::cleanup_connection(&connected.client);
    }
    (state, issue)
}

fn print_mcp_get_details(
    name: &str,
    config: &crate::services::mcp::types::ScopedMcpServerConfig,
    state: McpListState,
    issue: Option<&str>,
) {
    use crate::services::mcp::types::Transport;

    println!("{name}:");
    println!("  Scope: {}", mcp_scope_label(config.scope));
    println!("  Status: {}", state.as_get_status());
    if let Some(issue) = issue {
        println!("  Issue: {issue}");
    }

    if config.transport == Transport::Stdio {
        println!("  Type: stdio");
        println!("  Command: {}", config.command.as_deref().unwrap_or(""));
        println!("  Args: {}", config.args.join(" "));
        if !config.env.is_empty() {
            println!("  Environment:");
            for (key, value) in &config.env {
                println!("    {key}={value}");
            }
        }
    } else {
        println!("  Type: {}", config.transport.as_str());
        if let Some(url) = &config.url {
            println!("  URL: {url}");
        }
        if !config.headers.is_empty() {
            println!("  Headers:");
            for (key, value) in &config.headers {
                println!("    {key}: {value}");
            }
        }
        if let Some(oauth) = config.oauth.as_ref().and_then(serde_json::Value::as_object) {
            let mut oauth_details = Vec::new();
            if oauth.get("clientId").is_some() {
                oauth_details.push("client_id configured".to_string());
                if crate::services::mcp::auth::get_mcp_client_config(name, config)
                    .ok()
                    .flatten()
                    .is_some()
                {
                    oauth_details.push("client_secret configured".to_string());
                }
            }
            if let Some(callback_port) = oauth.get("callbackPort") {
                oauth_details.push(format!("callback_port {callback_port}"));
            }
            if !oauth_details.is_empty() {
                println!("  OAuth: {}", oauth_details.join(", "));
            }
        }
    }
    if let Some(timeout_ms) =
        crate::services::mcp::config::get_mcp_server_timeout_by_name_readonly(name)
    {
        let below_minimum = if timeout_ms < 1000 {
            " (ignored: below 1000ms minimum)"
        } else {
            ""
        };
        println!("  Timeout: {timeout_ms}ms{below_minimum}");
    }

    match config.scope {
        crate::services::mcp::types::ConfigScope::Local
        | crate::services::mcp::types::ConfigScope::Project
        | crate::services::mcp::types::ConfigScope::User => {
            println!();
            println!(
                "To remove this server, run: claude mcp remove {name} -s {}",
                config.scope.as_str()
            );
        }
        crate::services::mcp::types::ConfigScope::Enterprise => {
            if let Some(path) = mcp_scope_removal_path(config.scope) {
                println!();
                println!("To remove this server, edit {path}");
            }
        }
        crate::services::mcp::types::ConfigScope::Managed => {
            println!();
            println!(
                "This server is provided by your organization's managed settings and cannot be removed locally."
            );
        }
        crate::services::mcp::types::ConfigScope::Dynamic
        | crate::services::mcp::types::ConfigScope::ClaudeAi => {}
    }
}

fn run_mcp_get_cli(args: &[String]) -> anyhow::Result<()> {
    let [name] = args else {
        if args.is_empty() {
            anyhow::bail!("missing required argument 'name'; usage: cometix mcp get <name>");
        }
        anyhow::bail!(
            "unexpected argument '{}'; usage: cometix mcp get <name>",
            args[1]
        );
    };

    let config =
        crate::services::mcp::config::get_mcp_config_by_name_readonly(name).ok_or_else(|| {
            anyhow::anyhow!("No MCP server named \"{name}\". Run \"claude mcp add\" to add one.")
        })?;
    let project_config = crate::utils::config::get_current_project_config();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    crate::utils::process_runtime::set_process_runtime_handle(runtime.handle().clone());
    let (state, issue) = mcp_get_connection(&runtime, name, &config, &project_config);
    print_mcp_get_details(name, &config, state, issue.as_deref());
    Ok(())
}

fn run_mcp_list_cli(args: &[String]) -> anyhow::Result<()> {
    if !args.is_empty() {
        anyhow::bail!("unexpected argument '{}'; usage: cometix mcp list", args[0]);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    crate::utils::process_runtime::set_process_runtime_handle(runtime.handle().clone());
    let rows = mcp_list_rows(&runtime)?;
    if rows.is_empty() {
        println!("No MCP servers configured.");
        return Ok(());
    }

    println!("Checking MCP server health...\n");
    let results = runtime.block_on(async {
        futures::future::join_all(rows.iter().map(|row| async move {
            if let Some(state) = row.state {
                return (row.name.clone(), row.config.scope, state);
            }
            let connection =
                crate::services::mcp::client::connect_to_server(&row.name, &row.config).await;
            let state = match &connection {
                crate::services::mcp::types::McpServerConnection::Connected(_) => {
                    McpListState::Connected
                }
                crate::services::mcp::types::McpServerConnection::Failed(_) => McpListState::Failed,
                crate::services::mcp::types::McpServerConnection::NeedsAuth(_) => {
                    McpListState::NeedsAuth
                }
                crate::services::mcp::types::McpServerConnection::Pending(_) => {
                    McpListState::Pending
                }
                crate::services::mcp::types::McpServerConnection::Disabled(_) => {
                    McpListState::Disabled
                }
            };
            if let crate::services::mcp::types::McpServerConnection::Connected(connected) =
                &connection
            {
                crate::services::mcp::client::cleanup_connection(&connected.client);
            }
            (row.name.clone(), row.config.scope, state)
        }))
        .await
    });

    for (name, scope, state) in results {
        println!("{name} ({}) - {}", scope.as_str(), state.as_str());
    }
    Ok(())
}

fn run_mcp_add_json_cli(args: &[String]) -> anyhow::Result<(String, &'static str)> {
    let mut scope = crate::services::mcp::types::ConfigScope::Local;
    let mut positional = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == "-s" || arg == "--scope" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow::anyhow!("missing value for {arg}"))?;
            scope = parse_mcp_add_json_scope(value)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--scope=") {
            scope = parse_mcp_add_json_scope(value)?;
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("-s=") {
            scope = parse_mcp_add_json_scope(value)?;
            index += 1;
            continue;
        }
        if arg == "--client-secret" || arg.starts_with("--client-secret=") {
            anyhow::bail!("--client-secret is not supported by this build");
        }
        if arg != "-" && arg.starts_with('-') {
            anyhow::bail!("unknown option '{arg}'");
        }
        positional.push(arg.clone());
        index += 1;
    }

    if positional.len() != 2 {
        anyhow::bail!("expected <name> and <json|->");
    }
    let name = positional.remove(0);
    let payload = positional.remove(0);
    let server = read_mcp_add_json_payload(&payload)?;
    crate::services::mcp::config::add_mcp_server_config(&name, &server, scope)?;
    Ok((name, scope.as_str()))
}

fn parse_mcp_add_json_scope(
    value: &str,
) -> anyhow::Result<crate::services::mcp::types::ConfigScope> {
    match value {
        "local" => Ok(crate::services::mcp::types::ConfigScope::Local),
        "user" => Ok(crate::services::mcp::types::ConfigScope::User),
        "project" => Ok(crate::services::mcp::types::ConfigScope::Project),
        _ => anyhow::bail!("invalid scope '{value}'; expected local, user, or project"),
    }
}

fn read_mcp_add_json_payload(payload: &str) -> anyhow::Result<Value> {
    let text = if payload == "-" {
        let stdin = std::io::stdin();
        if stdin.is_terminal() {
            anyhow::bail!("There is no JSON on stdin. Pipe it in, or pass it as the argument.");
        }
        let mut bytes = Vec::new();
        stdin
            .take((MCP_ADD_JSON_MAX_STDIN_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MCP_ADD_JSON_MAX_STDIN_BYTES {
            anyhow::bail!("The JSON on stdin is over the limit of 1 MiB.");
        }
        String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("The JSON on stdin must be valid UTF-8"))?
    } else {
        payload.to_string()
    };

    if text.trim().is_empty() {
        anyhow::bail!("There is no JSON on stdin. Pipe it in, or pass it as the argument.");
    }
    let value = serde_json::from_str::<Value>(&text).map_err(|_| {
        anyhow::anyhow!(
            "Invalid configuration: not a JSON object. Check its quotes, commas and braces."
        )
    })?;
    if !value.is_object() {
        anyhow::bail!(
            "Invalid configuration: not a JSON object. Check its quotes, commas and braces."
        );
    }
    Ok(value)
}

fn cli_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::anyhow!(message.into())
}

fn option_value(args: &[String], flag: &str) -> Option<String> {
    let prefix = format!("{flag}=");
    for (index, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix(&prefix) {
            return Some(value.to_string());
        }
        if arg == flag {
            return args.get(index + 1).cloned();
        }
    }
    None
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn validate_setup_issuer(issuer: &str) -> anyhow::Result<()> {
    let trimmed = issuer.trim();
    let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| {
        cli_error(format!(
            "Error: --issuer must be a valid URL (got \"{issuer}\")"
        ))
    })?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if scheme.is_empty() || host.is_empty() || host.contains(char::is_whitespace) {
        return Err(cli_error(format!(
            "Error: --issuer must be a valid URL (got \"{issuer}\")"
        )));
    }
    let host_no_port = if let Some(stripped) = host.strip_prefix('[') {
        stripped
            .find(']')
            .map(|end| &stripped[..end])
            .unwrap_or(stripped)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    let loopback_http = scheme == "http"
        && matches!(
            host_no_port.to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "::1"
        );
    if scheme != "https" && !loopback_http {
        return Err(cli_error(format!(
            "Error: --issuer must use https:// (got \"{scheme}://{host}\")"
        )));
    }
    Ok(())
}

fn user_settings_path() -> anyhow::Result<std::path::PathBuf> {
    get_settings_file_path_for_source(SettingSource::User)
        .ok_or_else(|| anyhow::anyhow!("user settings path is unavailable"))
}

fn read_user_settings_value() -> anyhow::Result<Value> {
    let path = user_settings_path()?;
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(serde_json::from_str::<Value>(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(error.into()),
    }
}

fn write_user_settings_value(value: &Value) -> anyhow::Result<()> {
    let path = user_settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn update_user_xaa_idp(
    settings: Option<crate::utils::settings::types::XaaIdpSettings>,
) -> anyhow::Result<()> {
    // Maps to: CC `updateSettingsForSource('userSettings', { xaaIdp: ... })`.
    let mut value = read_user_settings_value()?;
    if !value.is_object() {
        value = serde_json::json!({});
    }
    let object = value.as_object_mut().expect("object checked above");
    if let Some(settings) = settings {
        let mut xaa = serde_json::Map::new();
        xaa.insert("issuer".to_string(), Value::String(settings.issuer));
        xaa.insert("clientId".to_string(), Value::String(settings.client_id));
        if let Some(port) = settings.callback_port {
            xaa.insert(
                "callbackPort".to_string(),
                Value::Number(serde_json::Number::from(port)),
            );
        }
        object.insert("xaaIdp".to_string(), Value::Object(xaa));
    } else {
        object.remove("xaaIdp");
    }
    write_user_settings_value(&value)
}

fn parse_callback_port(args: &[String]) -> anyhow::Result<Option<u16>> {
    let Some(raw) = option_value(args, "--callback-port") else {
        return Ok(None);
    };
    raw.parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .map(Some)
        .ok_or_else(|| cli_error("Error: --callback-port must be a positive integer"))
}

fn format_iso_millis(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn run_xaa_setup(args: &[String]) -> anyhow::Result<()> {
    let issuer = option_value(args, "--issuer")
        .ok_or_else(|| cli_error("Error: missing required option '--issuer <url>'"))?;
    let client_id = option_value(args, "--client-id")
        .ok_or_else(|| cli_error("Error: missing required option '--client-id <id>'"))?;
    validate_setup_issuer(&issuer)?;
    let callback_port = parse_callback_port(args)?;
    let secret = if has_flag(args, "--client-secret") {
        Some(
            crate::utils::process_env::var("MCP_XAA_IDP_CLIENT_SECRET")
                .truthy()
                .ok_or_else(|| {
                    cli_error("Error: --client-secret requires MCP_XAA_IDP_CLIENT_SECRET env var")
                })?,
        )
    } else {
        None
    };

    let old = crate::services::mcp::xaa_idp_login::get_xaa_idp_settings();
    let old_issuer = old.as_ref().map(|value| value.issuer.clone());
    let old_client_id = old.as_ref().map(|value| value.client_id.clone());

    update_user_xaa_idp(Some(crate::utils::settings::types::XaaIdpSettings {
        issuer: issuer.clone(),
        client_id: client_id.clone(),
        callback_port,
    }))
    .map_err(|error| cli_error(format!("Error writing settings: {error}")))?;

    if let Some(old_issuer) = old_issuer {
        if crate::services::mcp::xaa_idp_login::issuer_key(&old_issuer)
            != crate::services::mcp::xaa_idp_login::issuer_key(&issuer)
        {
            crate::services::mcp::xaa_idp_login::clear_idp_id_token(&old_issuer)?;
            crate::services::mcp::xaa_idp_login::clear_idp_client_secret(&old_issuer)?;
        } else if old_client_id.as_deref() != Some(client_id.as_str()) {
            crate::services::mcp::xaa_idp_login::clear_idp_id_token(&old_issuer)?;
            crate::services::mcp::xaa_idp_login::clear_idp_client_secret(&old_issuer)?;
        }
    }

    if let Some(secret) = secret {
        crate::services::mcp::xaa_idp_login::save_idp_client_secret(&issuer, &secret).map_err(|error| {
            cli_error(format!(
                "Error: settings written but keychain save failed — {error}. Re-run with --client-secret once keychain is available."
            ))
        })?;
    }
    println!("XAA IdP connection configured for {issuer}");
    Ok(())
}

fn run_xaa_login(args: &[String], rt: &tokio::runtime::Runtime) -> anyhow::Result<()> {
    let idp = crate::services::mcp::xaa_idp_login::get_xaa_idp_settings().ok_or_else(|| {
        cli_error("Error: no XAA IdP connection. Run 'claude mcp xaa setup' first.")
    })?;
    if let Some(id_token) = option_value(args, "--id-token") {
        let expires_at = crate::services::mcp::xaa_idp_login::save_idp_id_token_from_jwt(
            &idp.issuer,
            &id_token,
        )?;
        println!(
            "id_token cached for {} (expires {})",
            idp.issuer,
            format_iso_millis(expires_at)
        );
        return Ok(());
    }
    if has_flag(args, "--force") {
        crate::services::mcp::xaa_idp_login::clear_idp_id_token(&idp.issuer)?;
    }
    if crate::services::mcp::xaa_idp_login::get_cached_idp_id_token(&idp.issuer).is_some() {
        println!(
            "Already logged in to {} (cached id_token still valid). Use --force to re-login.",
            idp.issuer
        );
        return Ok(());
    }
    println!("Opening browser for IdP login at {}…", idp.issuer);
    let issuer_for_callback = idp.issuer.clone();
    let result = rt.block_on(crate::services::mcp::xaa_idp_login::acquire_idp_id_token(
        crate::services::mcp::xaa_idp_login::IdpLoginOptions {
            idp_issuer: idp.issuer.clone(),
            idp_client_id: idp.client_id,
            idp_client_secret: crate::services::mcp::xaa_idp_login::get_idp_client_secret(
                &idp.issuer,
            ),
            callback_port: idp.callback_port,
            on_authorization_url: Some(std::sync::Arc::new(move |url| {
                let _ = &issuer_for_callback;
                println!("If the browser did not open, visit:\n  {url}");
            })),
            skip_browser_open: false,
        },
    ));
    match result {
        Ok(_) => {
            println!("Logged in. MCP servers with --xaa will now authenticate silently.");
            Ok(())
        }
        Err(error) => Err(cli_error(format!("IdP login failed: {error}"))),
    }
}

fn run_xaa_show() -> anyhow::Result<()> {
    let Some(idp) = crate::services::mcp::xaa_idp_login::get_xaa_idp_settings() else {
        println!("No XAA IdP connection configured.");
        return Ok(());
    };
    let has_secret =
        crate::services::mcp::xaa_idp_login::get_idp_client_secret(&idp.issuer).is_some();
    let has_id_token =
        crate::services::mcp::xaa_idp_login::get_cached_idp_id_token(&idp.issuer).is_some();
    println!("Issuer:        {}", idp.issuer);
    println!("Client ID:     {}", idp.client_id);
    if let Some(port) = idp.callback_port {
        println!("Callback port: {port}");
    }
    println!(
        "Client secret: {}",
        if has_secret {
            "(stored in keychain)"
        } else {
            "(not set — PKCE-only)"
        }
    );
    println!(
        "Logged in:     {}",
        if has_id_token {
            "yes (id_token cached)"
        } else {
            "no — run 'claude mcp xaa login'"
        }
    );
    Ok(())
}

fn run_xaa_clear() -> anyhow::Result<()> {
    let idp = crate::services::mcp::xaa_idp_login::get_xaa_idp_settings();
    update_user_xaa_idp(None)
        .map_err(|error| cli_error(format!("Error writing settings: {error}")))?;
    if let Some(idp) = idp {
        crate::services::mcp::xaa_idp_login::clear_idp_id_token(&idp.issuer)?;
        crate::services::mcp::xaa_idp_login::clear_idp_client_secret(&idp.issuer)?;
    }
    println!("XAA IdP connection cleared");
    Ok(())
}

fn run_mcp_xaa_cli(args: &[String], rt: &tokio::runtime::Runtime) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("setup") => run_xaa_setup(&args[1..]),
        Some("login") => run_xaa_login(&args[1..], rt),
        Some("show") => run_xaa_show(),
        Some("clear") => run_xaa_clear(),
        Some(other) => Err(cli_error(format!("error: unknown command '{other}'"))),
        None => Err(cli_error(
            "error: missing XAA command (expected setup, login, show, or clear)",
        )),
    }
}

fn parts(args: &str) -> Vec<&str> {
    args.trim().split_whitespace().collect()
}

/// Returns the official `onComplete(...)` output for the current no-client MCP
/// state. This mirrors the visible strings from `MCPSettings`, `MCPToggle`, and
/// `MCPReconnect` without instantiating MCP clients or touching settings.
#[cfg(test)]
fn local_output_for_args(args: &str) -> String {
    let parts = parts(args);
    match parts.as_slice() {
        [] | ["no-redirect"] => NO_MCP_SERVERS_CONFIGURED.to_string(),
        ["reconnect", rest @ ..] if !rest.is_empty() => {
            format!("MCP server \"{}\" not found", rest.join(" "))
        }
        ["enable"] => "All MCP servers are already enabled".to_string(),
        ["disable"] => "All MCP servers are already disabled".to_string(),
        ["enable", rest @ ..] | ["disable", rest @ ..] if !rest.is_empty() => {
            let target = rest.join(" ");
            if target == "all" {
                if parts[0] == "enable" {
                    "All MCP servers are already enabled".to_string()
                } else {
                    "All MCP servers are already disabled".to_string()
                }
            } else {
                format!("MCP server \"{target}\" not found")
            }
        }
        _ => NO_MCP_SERVERS_CONFIGURED.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct McpTogglePlan {
    names: Vec<String>,
    result: String,
}

/// Maps to: CC `commands/mcp/mcp.tsx#MCPToggle` target selection and
/// `onComplete(...)` string generation.
fn mcp_toggle_plan_for_clients(
    action: &str,
    target: &str,
    clients: &[McpServerConnection],
) -> McpTogglePlan {
    let is_enabling = action == "enable";
    let clients = clients
        .iter()
        .filter(|client| client.name() != "ide")
        .collect::<Vec<_>>();
    let names = if target == "all" {
        clients
            .iter()
            .filter(|client| {
                if is_enabling {
                    client.connection_type() == McpServerConnectionType::Disabled
                } else {
                    client.connection_type() != McpServerConnectionType::Disabled
                }
            })
            .map(|client| client.name().to_owned())
            .collect::<Vec<_>>()
    } else {
        clients
            .iter()
            .filter(|client| client.name() == target)
            .map(|client| client.name().to_owned())
            .collect::<Vec<_>>()
    };

    let result = if names.is_empty() {
        if target == "all" {
            format!(
                "All MCP servers are already {}",
                if is_enabling { "enabled" } else { "disabled" }
            )
        } else {
            format!("MCP server \"{target}\" not found")
        }
    } else if target == "all" {
        format!(
            "{} {} MCP server(s)",
            if is_enabling { "Enabled" } else { "Disabled" },
            names.len()
        )
    } else {
        format!(
            "MCP server \"{target}\" {}",
            if is_enabling { "enabled" } else { "disabled" }
        )
    };

    McpTogglePlan { names, result }
}

fn dispatch_mcp_toggle_plan(
    plan: &McpTogglePlan,
    toggle_mcp_server: crate::services::mcp::mcp_connection_manager::ToggleMcpServer,
) {
    if plan.names.is_empty() {
        return;
    }

    for name in plan.names.clone() {
        // Maps to: CC `commands/mcp/mcp.tsx:21,` — dispatch through
        // `useMcpToggleEnabled()`. The manager owns config lookup (cached
        // getGlobalConfig-equivalent), pending update, persisted enable
        // mutation, and reconnect/disable lifecycle.
        let toggle_mcp_server = toggle_mcp_server.clone();
        tokio::spawn(async move {
            let _ = toggle_mcp_server.call(&name).await;
        });
    }
}

#[derive(Default, Props)]
pub struct McpCommandPanelProps<'a> {
    pub args: String,
    pub on_close: HandlerMut<'a, ()>,
    pub on_result: HandlerMut<'a, String>,
}

#[component]
pub fn McpCommandPanel<'a>(
    props: &mut McpCommandPanelProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // Maps to: CC `commands/mcp/mcp.tsx:20-21` — `useAppState(s => s.mcp.clients)`
    // for the read and `useMcpToggleEnabled()` for the write. There is no
    // `useMcp` at the source.
    let mcp_clients =
        crate::state::app_state::use_app_state(&mut hooks, |state| state.mcp.clients.clone());
    let toggle_mcp_server =
        crate::services::mcp::mcp_connection_manager::use_mcp_toggle_enabled(&mut hooks);
    let mut pending_result = hooks.use_state(|| Option::<String>::None);
    let mut did_run_effect = hooks.use_state(|| false);

    let pending_result_value = { pending_result.read().clone() };
    if let Some(result) = pending_result_value {
        pending_result.set(None);
        (props.on_result)(result);
    }

    let args = props.args.trim().to_string();
    let arg_parts = parts(&args);

    if arg_parts.is_empty() || arg_parts == ["no-redirect"] {
        let complete_state = pending_result;
        return element! {
            MCPSettings(on_complete: move |result: String| {
                let mut complete_state = complete_state;
                complete_state.set(Some(result));
            })
        }
        .into_any();
    }

    if let ["reconnect", rest @ ..] = arg_parts.as_slice() {
        if !rest.is_empty() {
            let complete_state = pending_result;
            return element! {
                MCPReconnect(
                    server_name: rest.join(" "),
                    on_complete: move |result: String| {
                        let mut complete_state = complete_state;
                        complete_state.set(Some(result));
                    },
                )
            }
            .into_any();
        }
    }

    if matches!(arg_parts.first().copied(), Some("enable" | "disable")) {
        let action = arg_parts[0];
        let target = if arg_parts.len() > 1 {
            arg_parts[1..].join(" ")
        } else {
            "all".to_string()
        };
        let plan = mcp_toggle_plan_for_clients(action, &target, &mcp_clients);
        if !did_run_effect.get() {
            did_run_effect.set(true);
            dispatch_mcp_toggle_plan(&plan, toggle_mcp_server);
            (props.on_result)(plan.result.clone());
        }
        return element! { View {} }.into_any();
    }

    let complete_state = pending_result;
    element! {
        MCPSettings(on_complete: move |result: String| {
            let mut complete_state = complete_state;
            complete_state.set(Some(result));
        })
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_env::{EnvVarGuard, TEST_ENV_LOCK};
    use crate::utils::theme;
    use futures::{StreamExt, stream};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn with_temp_config_home(test: impl FnOnce(&std::path::Path)) {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "cometix-mcp-xaa-command-test-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _config_guard = EnvVarGuard::set("CLAUDE_CONFIG_DIR", &dir);
        test(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_get_issue_redaction_removes_normalized_url_credentials_and_auth_values() {
        let url_password = "url-password";
        let mut config =
            crate::services::mcp::types::ScopedMcpServerConfig::stdio_for_test("redact");
        config.url = Some(format!(
            "http://url-user:{url_password}@example.invalid/private-path?access_token=url-secret"
        ));
        config
            .headers
            .insert("Authorization".into(), "Bearer static-secret".into());
        config.env.insert("TOKEN".into(), "env-secret".into());
        config.auth_token = Some("auth-secret".into());
        let error = format!(
            "dial http://url-user:{url_password}@example.invalid/private-path?access_token=url-secret Authorization: Bearer dynamic-secret TOKEN=env-secret auth-secret"
        );

        let redacted = redact_mcp_issue(&error, &config);
        for secret in [
            "url-user",
            url_password,
            "private-path",
            "url-secret",
            "static-secret",
            "dynamic-secret",
            "env-secret",
            "auth-secret",
        ] {
            assert!(!redacted.contains(secret), "{secret} leaked in {redacted}");
        }
        assert!(redacted.contains("<redacted URL>"), "{redacted}");
        assert!(redacted.contains("Authorization=REDACTED"), "{redacted}");
    }

    #[test]
    fn mcp_get_issue_redaction_covers_url_normalization() {
        let url_password = "url-password";
        let mut config =
            crate::services::mcp::types::ScopedMcpServerConfig::stdio_for_test("redact");
        config.url = Some(format!(
            "http://User:{url_password}@EXAMPLE.invalid/private-path?access_token=url-secret"
        ));
        let error = format!(
            "dial http://user:{url_password}@example.invalid/private-path?access_token=url-secret"
        );

        let redacted = redact_mcp_issue(&error, &config);
        assert!(!redacted.contains(url_password), "{redacted}");
        assert!(!redacted.contains("url-secret"), "{redacted}");
        assert!(!redacted.contains("private-path"), "{redacted}");
        assert!(redacted.contains("access_token=REDACTED"), "{redacted}");
    }

    fn canvas_text(canvas: &Canvas) -> String {
        canvas.to_string()
    }

    fn app_state_with_mcp_clients(
        clients: Vec<McpServerConnection>,
    ) -> crate::state::app_state_store::AppState {
        crate::state::app_state_store::AppState {
            mcp: Arc::new(crate::state::app_state_store::McpState {
                clients,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn xaa_setup_validation_matches_official_https_and_loopback_rules() {
        assert!(validate_setup_issuer("https://idp.example.com/tenant").is_ok());
        assert!(validate_setup_issuer("http://localhost:8080/issuer").is_ok());
        assert!(validate_setup_issuer("http://127.0.0.1:8080/issuer").is_ok());
        assert!(validate_setup_issuer("http://[::1]:8080/issuer").is_ok());
        let error = validate_setup_issuer("http://idp.example.com/issuer")
            .expect_err("remote plaintext issuer must be rejected");
        assert!(
            error.to_string().contains("must use https://"),
            "error={error}"
        );
    }

    #[test]
    fn xaa_setup_writes_user_settings_and_removes_stale_callback_port() {
        with_temp_config_home(|home| {
            std::fs::write(
                home.join("settings.json"),
                r#"{"model":"sonnet","xaaIdp":{"issuer":"https://old.example.com/","clientId":"old","callbackPort":3118}}"#,
            )
            .unwrap();

            run_xaa_setup(&[
                "--issuer".to_string(),
                "https://IDP.example.com/tenant/".to_string(),
                "--client-id".to_string(),
                "cc-id".to_string(),
            ])
            .unwrap();

            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(home.join("settings.json")).unwrap())
                    .unwrap();
            assert_eq!(
                value.get("model").and_then(serde_json::Value::as_str),
                Some("sonnet")
            );
            let xaa = value.get("xaaIdp").expect("xaaIdp written");
            assert_eq!(
                xaa.get("issuer").and_then(serde_json::Value::as_str),
                Some("https://IDP.example.com/tenant/")
            );
            assert_eq!(
                xaa.get("clientId").and_then(serde_json::Value::as_str),
                Some("cc-id")
            );
            assert!(
                xaa.get("callbackPort").is_none(),
                "callbackPort must not leak from old setup: {xaa}"
            );
        });
    }

    #[test]
    fn mcp_outputs_match_official_no_client_on_complete_paths() {
        assert_eq!(local_output_for_args(""), NO_MCP_SERVERS_CONFIGURED);
        assert_eq!(
            local_output_for_args("no-redirect"),
            NO_MCP_SERVERS_CONFIGURED
        );
        assert_eq!(
            local_output_for_args("enable"),
            "All MCP servers are already enabled"
        );
        assert_eq!(
            local_output_for_args("disable"),
            "All MCP servers are already disabled"
        );
        assert_eq!(
            local_output_for_args("enable docs server"),
            "MCP server \"docs server\" not found"
        );
        assert_eq!(
            local_output_for_args("reconnect docs"),
            "MCP server \"docs\" not found"
        );
    }

    #[test]
    fn mcp_toggle_plan_matches_official_mcp_toggle_selection_and_copy() {
        use crate::services::mcp::types::{ScopedMcpServerConfig, Transport};

        let ide_config = ScopedMcpServerConfig {
            transport: Transport::SseIde,
            command: None,
            url: Some("http://127.0.0.1:1234/sse".to_string()),
            ide_name: Some("IDE".to_string()),
            ..ScopedMcpServerConfig::stdio_for_test("unused")
        };
        let clients = vec![
            McpServerConnection::connected_for_test(
                "enabled",
                ScopedMcpServerConfig::stdio_for_test("enabled-mcp"),
            ),
            McpServerConnection::disabled(
                "disabled",
                ScopedMcpServerConfig::stdio_for_test("disabled-mcp"),
            ),
            McpServerConnection::connected_for_test("ide", ide_config),
        ];

        assert_eq!(
            mcp_toggle_plan_for_clients("enable", "all", &clients),
            McpTogglePlan {
                names: vec!["disabled".to_string()],
                result: "Enabled 1 MCP server(s)".to_string(),
            }
        );
        assert_eq!(
            mcp_toggle_plan_for_clients("disable", "all", &clients),
            McpTogglePlan {
                names: vec!["enabled".to_string()],
                result: "Disabled 1 MCP server(s)".to_string(),
            }
        );
        assert_eq!(
            mcp_toggle_plan_for_clients("enable", "enabled", &clients),
            McpTogglePlan {
                names: vec!["enabled".to_string()],
                result: "MCP server \"enabled\" enabled".to_string(),
            }
        );
        assert_eq!(
            mcp_toggle_plan_for_clients("disable", "missing", &clients).result,
            "MCP server \"missing\" not found"
        );
    }

    /// Renders `/mcp <args>` over the default AppState, which has no MCP
    /// clients, and returns what it completed with and the last canvas.
    fn render_panel_without_clients(args: &str) -> (Vec<String>, String) {
        let results = Arc::new(Mutex::new(Vec::<String>::new()));
        let results_for_handler = Arc::clone(&results);
        let last = Arc::new(Mutex::new(String::new()));
        let last_for_loop = Arc::clone(&last);
        let args = args.to_string();

        futures::executor::block_on(async move {
            let current_theme = *theme::current();
            let mut app = element! {
                ContextProvider(value: Context::owned(current_theme)) {
                    crate::state::app_state::AppStateProvider(
                        children: crate::state::app_state::ProviderChildren::new(move || {
                            let results_for_handler = Arc::clone(&results_for_handler);
                            element! {
                                crate::services::mcp::mcp_connection_manager::McpConnectionManager() {
                                    McpCommandPanel(
                                        args: args.clone(),
                                        on_result: move |result: String| results_for_handler.lock().expect("results mutex").push(result),
                                    )
                                }
                            }.into_any()
                        }),
                    )
                }
            };
            let mut render_loop = Box::pin(
                app.mock_terminal_render_loop(MockTerminalConfig::default().with_size(120, 24)),
            );
            for _ in 0..6 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                let Some(canvas) = next else {
                    break;
                };
                *last_for_loop.lock().expect("last canvas mutex") = canvas.to_string();
            }
        });

        let results = results.lock().expect("results mutex").clone();
        let last = last.lock().expect("last canvas mutex").clone();
        (results, last)
    }

    #[test]
    fn mcp_panel_no_servers_completes_with_official_message_without_ui_only_panel() {
        // A server configured but not in `mcp.clients` is not listed: the
        // settings view reads AppState only, as CC's does.
        let _env_guard = TEST_ENV_LOCK.lock().unwrap();
        let mut global_config = crate::utils::config::GlobalConfig::default();
        global_config.mcp_servers = Some(serde_json::json!({"docs":{"command":"docs-mcp"}}));
        crate::utils::config::set_test_global_config(Some(global_config));

        let (results, last) = render_panel_without_clients("");
        crate::utils::config::set_test_global_config(None);

        assert_eq!(results, [NO_MCP_SERVERS_CONFIGURED.to_string()]);
        assert!(!last.contains("UI-only build"), "canvas=\n{last}");
        assert!(!last.contains("Manage MCP servers"), "canvas=\n{last}");
    }

    /// CC `commands/mcp/mcp.tsx:20`: `/mcp enable|disable` reads
    /// `s.mcp.clients` and nothing else.
    #[test]
    fn mcp_toggle_without_clients_reports_the_server_not_found() {
        let (results, _) = render_panel_without_clients("disable docs");
        assert_eq!(results, ["MCP server \"docs\" not found".to_string()]);
    }

    #[test]
    fn mcp_panel_unknown_args_falls_back_to_live_settings_state_like_official_call() {
        let _env_guard = TEST_ENV_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("cometix-mcp-panel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // MCPSettings reads agent definitions from the cwd; a scratch cwd keeps
        // the repository's own `.claude/agents` out of the server list.
        struct CwdGuard(std::path::PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        let _cwd = CwdGuard(std::env::current_dir().unwrap());
        std::env::set_current_dir(&temp_dir).unwrap();

        let current_theme = *theme::current();
        let initial_state = app_state_with_mcp_clients(vec![McpServerConnection::pending(
            "docs",
            crate::services::mcp::types::ScopedMcpServerConfig::stdio_for_test("docs-mcp"),
        )]);

        let canvas = element! {
            ContextProvider(value: Context::owned(current_theme)) {
                // AppStateProvider is OUTSIDE the manager, matching
                // production nesting (Provider -> REPL -> manager). The
                // configured server is already a `pending` record in
                // `mcp.clients`, as `initializeServersAsPending` leaves it
                // before the settings view can render.
                crate::state::app_state::AppStateProvider(
                    initial_state: Some(initial_state),
                    children: crate::state::app_state::ProviderChildren::new(|| element! {
                        crate::services::mcp::mcp_connection_manager::McpConnectionManager() {
                            McpCommandPanel(args: "unexpected".to_string())
                        }
                    }.into_any()),
                )
            }
        }
        .render(Some(120));
        let text = canvas_text(&canvas);

        assert!(text.contains("Manage MCP servers"), "canvas=\n{text}");
        assert!(text.contains("1 server"), "canvas=\n{text}");
        assert!(text.contains("docs"), "canvas=\n{text}");
        assert!(!text.contains("0 servers"), "canvas=\n{text}");
        assert_eq!(
            text.matches("↑↓ to navigate").count(),
            1,
            "the command wrapper must not duplicate MCPListPanel's footer: canvas=\n{text}"
        );

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn mcp_panel_esc_completes_through_mcp_settings_without_outer_footer_or_cancel_owner() {
        let current_theme = *theme::current();
        let close_count = Arc::new(Mutex::new(0usize));
        let close_for_handler = Arc::clone(&close_count);
        let results = Arc::new(Mutex::new(Vec::<String>::new()));
        let results_for_handler = Arc::clone(&results);
        let initial_state = app_state_with_mcp_clients(vec![McpServerConnection::pending(
            "docs",
            crate::services::mcp::types::ScopedMcpServerConfig::stdio_for_test("docs-mcp"),
        )]);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(
                    crate::keybindings::keybinding_context::KeybindingRuntime::with_default_bindings()
                )) {
                    ContextProvider(value: Context::owned(current_theme)) {
                        // The pending client this test drives Esc against is
                        // a `mcp.clients` record, which is all the settings
                        // view reads (CC `MCPSettings.tsx:32-34`).
                        crate::state::app_state::AppStateProvider(
                            initial_state: Some(initial_state),
                            children: crate::state::app_state::ProviderChildren::new(move || {
                                let close_for_handler = Arc::clone(&close_for_handler);
                                let results_for_handler = Arc::clone(&results_for_handler);
                                element! {
                                    crate::services::mcp::mcp_connection_manager::McpConnectionManager() {
                                        McpCommandPanel(
                                        args: String::new(),
                                        on_close: move |_| {
                                            *close_for_handler.lock().expect("close mutex") += 1;
                                        },
                                        on_result: move |result| {
                                            results_for_handler.lock().expect("result mutex").push(result);
                                        },
                                        )
                                    }
                                }.into_any()
                            }),
                        )
                    }
                }
            };
            let mut render_loop = Box::pin(
                app.mock_terminal_render_loop(
                    MockTerminalConfig::with_events(stream::iter(vec![TerminalEvent::Key(
                        KeyEvent::new(KeyEventKind::Press, KeyCode::Esc),
                    )]))
                    .with_size(100, 24),
                ),
            );
            for _ in 0..8 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                if next.is_none() {
                    break;
                }
            }
        });

        assert_eq!(*close_count.lock().expect("close mutex"), 0);
        assert_eq!(
            results.lock().expect("result mutex").as_slice(),
            &["MCP dialog dismissed".to_string()]
        );
    }
}
