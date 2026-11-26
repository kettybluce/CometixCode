//! Maps to: CC 2.1.332 `cli/auth` plus `commands/auth/*`.
//!
//! `auth status` is a read-only shell command. `auth logout` performs the
//! explicit, user-requested credential deletion but does not attempt the
//! network revocation that Claude Code performs when it can reach Anthropic.
//! `auth login` keeps the existing product-wide credential-write gate closed:
//! its help and argument/error contract are reproduced, while the browser flow
//! reports the same unavailable reason as the in-session login UI.

use crate::services::analytics::config::is_analytics_disabled;
use crate::utils::auth::{
    ApiKeySource, AuthTokenSource, GetAnthropicApiKeyOptions, claude_ai_subscription_name,
    get_anthropic_api_key_with_source, get_auth_token_source,
};
use crate::utils::env_utils::{get_claude_config_home_dir, is_running_on_homespace};
use crate::utils::model::providers::{ApiProvider, get_api_provider};
use crate::utils::process_env::JsTruthy;
use crate::utils::session_storage::get_projects_dir;
use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value};

const ROOT_HELP: &str = "Usage: cometix auth [options] [command]

Manage authentication

Options:
  -h, --help        Display help for command

Commands:
  help [command]    display help for command
  login [options]   Sign in to your Anthropic account
  logout [options]  Log out from your Anthropic account
  status [options]  Show authentication status";

const LOGIN_HELP: &str = "Usage: cometix auth login [options]

Sign in to your Anthropic account

Options:
  --claudeai       Use Claude subscription (default)
  --console        Use Anthropic Console (API usage billing) instead of Claude
                   subscription
  --email <email>  Fill in the email address on the login page
  -h, --help       Display help for command
  --sso            Log in with SSO";

const LOGOUT_HELP: &str = "Usage: cometix auth logout [options]

Log out from your Anthropic account

Options:
  -h, --help  Display help for command";

const STATUS_HELP: &str = "Usage: cometix auth status [options]

Show authentication status

Options:
  -h, --help  Display help for command
  --json      Output as JSON (default)
  --text      Output as plain text";

const LOGOUT_SUCCESS: &str = "Successfully logged out from your Anthropic account.";
const LOGOUT_UNCONFIRMED: &str = "Logged out on this computer, but Anthropic didn't confirm the login was revoked. It may still work until you revoke it at https://claude.ai/settings/claude-code";
const LOGOUT_REVOKE_URL: &str = "https://claude.ai/settings/claude-code";
const LOGIN_UNAVAILABLE: &str =
    "OAuth login is unavailable in this build (credential side effects are disabled).";

/// Maps to: CC `auth` commander entry. Returns `None` when argv is not this
/// subcommand.
pub fn maybe_run_auth_cli(argv: &[String]) -> Option<i32> {
    let auth_index = argv.iter().position(|arg| arg == "auth")?;
    let mut prefix_settings = None;
    let mut command = None;
    let mut command_args = &[][..];
    let mut options_done = false;
    let mut unknown_option: Option<&str> = None;
    let mut index = auth_index + 1;

    while index < argv.len() {
        let arg = argv[index].as_str();
        if !options_done {
            if arg == "--" {
                options_done = true;
                index += 1;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                println!("{ROOT_HELP}");
                return Some(0);
            }
            if let Some(value) = arg.strip_prefix("--settings=") {
                prefix_settings = Some(value.to_string());
                index += 1;
                continue;
            }
            if arg == "--settings" {
                let Some(value) = argv.get(index + 1) else {
                    eprintln!("error: option '--settings <file-or-json>' argument missing");
                    return Some(1);
                };
                prefix_settings = Some(value.clone());
                index += 2;
                continue;
            }
            if arg.starts_with('-') {
                unknown_option.get_or_insert(arg);
                index += 1;
                continue;
            }
        }
        command = Some(arg);
        command_args = &argv[index + 1..];
        break;
    }

    if let Some(option) = unknown_option {
        eprintln!("error: unknown option '{option}'");
        return Some(1);
    }

    let Some(command) = command else {
        eprintln!("{ROOT_HELP}");
        return Some(1);
    };

    let result = match command {
        "help" => run_help(command_args),
        "login" => run_login(command_args, prefix_settings),
        "logout" => run_logout(command_args, prefix_settings),
        "status" => run_status(command_args, prefix_settings),
        other => {
            eprintln!("error: unknown command '{other}'");
            1
        }
    };
    Some(result)
}

fn run_help(args: &[String]) -> i32 {
    let target = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str);
    match target {
        None | Some("auth") => {
            println!("{ROOT_HELP}");
            0
        }
        Some("login") => {
            println!("{LOGIN_HELP}");
            0
        }
        Some("logout") => {
            println!("{LOGOUT_HELP}");
            0
        }
        Some("status") => {
            println!("{STATUS_HELP}");
            0
        }
        Some(_) => {
            eprintln!("{ROOT_HELP}");
            1
        }
    }
}

fn resolve_settings(settings: Option<String>) -> Result<(), i32> {
    let Some(value) = settings else {
        return Ok(());
    };
    crate::main::load_settings_from_flag(&value).map_err(|error| {
        eprintln!("{error}");
        1
    })
}

/// Applies one `--settings` token inside a subcommand. Returns `Ok(true)` when
/// the token was consumed, `Ok(false)` when it was not, and `Err(())` after
/// printing the commander-shaped error.
fn take_subcommand_settings(
    args: &[String],
    index: &mut usize,
    settings: &mut Option<String>,
) -> Result<bool, ()> {
    let arg = args[*index].as_str();
    if let Some(value) = arg.strip_prefix("--settings=") {
        *settings = Some(value.to_string());
        *index += 1;
        return Ok(true);
    }
    if arg == "--settings" {
        let Some(value) = args.get(*index + 1) else {
            // Commander accepts the global option after a subcommand only
            // when it carries a value; the bare token is reported as unknown.
            eprintln!("error: unknown option '--settings'");
            return Err(());
        };
        *settings = Some(value.clone());
        *index += 2;
        return Ok(true);
    }
    Ok(false)
}

fn help_requested(args: &[String], skip_email_value: bool) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return false;
        }
        if arg == "-h" || arg == "--help" {
            return true;
        }
        if arg == "--settings" || (skip_email_value && arg == "--email") {
            index += 2;
            continue;
        }
        index += 1;
    }
    false
}

fn run_status(args: &[String], mut settings: Option<String>) -> i32 {
    if help_requested(args, false) {
        println!("{STATUS_HELP}");
        return 0;
    }
    let mut text = false;
    let mut options_done = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if !options_done {
            if arg == "--" {
                options_done = true;
                index += 1;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                println!("{STATUS_HELP}");
                return 0;
            }
            match take_subcommand_settings(args, &mut index, &mut settings) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(()) => return 1,
            }
            if arg == "--text" {
                text = true;
            } else if arg == "--json" {
                // JSON is the default. `--text` wins regardless of order.
            } else if arg.starts_with('-') {
                eprintln!("error: unknown option '{arg}'");
                return 1;
            }
        }
        index += 1;
    }
    if let Err(code) = resolve_settings(settings) {
        return code;
    }
    let status = collect_status();
    if text {
        println!("{}", status.text());
    } else {
        println!("{}", status.json());
    }
    if status.logged_in { 0 } else { 1 }
}

fn run_logout(args: &[String], mut settings: Option<String>) -> i32 {
    if help_requested(args, false) {
        println!("{LOGOUT_HELP}");
        return 0;
    }
    let mut json = false;
    let mut options_done = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if !options_done {
            if arg == "--" {
                options_done = true;
                index += 1;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                println!("{LOGOUT_HELP}");
                return 0;
            }
            match take_subcommand_settings(args, &mut index, &mut settings) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(()) => return 1,
            }
            if arg == "--json" {
                json = true;
            } else if arg.starts_with('-') {
                eprintln!("error: unknown option '{arg}'");
                return 1;
            }
        }
        index += 1;
    }
    if let Err(code) = resolve_settings(settings) {
        return code;
    }
    let outcome = match crate::commands::logout::logout::perform_logout_status(false) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("Couldn't log out: {error}");
            return 1;
        }
    };
    let message = if outcome.removed_account_credentials {
        LOGOUT_UNCONFIRMED
    } else {
        LOGOUT_SUCCESS
    };
    if json {
        let mut output = Map::new();
        output.insert("message".to_string(), Value::String(message.to_string()));
        if outcome.removed_account_credentials {
            output.insert(
                "revokeUrl".to_string(),
                Value::String(LOGOUT_REVOKE_URL.to_string()),
            );
        }
        println!(
            "{}",
            serde_json::to_string(&Value::Object(output)).unwrap_or_default()
        );
    } else {
        println!("{message}");
    }
    0
}

fn run_login(args: &[String], mut settings: Option<String>) -> i32 {
    if help_requested(args, true) {
        println!("{LOGIN_HELP}");
        return 0;
    }
    let mut email = None;
    let mut sso = false;
    let mut console = false;
    let mut claudeai = false;
    let mut gateway_json = false;
    let mut organization_gateway = false;
    let mut ldr = false;
    let mut options_done = false;
    let mut index = 0;

    while index < args.len() {
        let arg = args[index].as_str();
        if !options_done {
            if arg == "--" {
                options_done = true;
                index += 1;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                println!("{LOGIN_HELP}");
                return 0;
            }
            match take_subcommand_settings(args, &mut index, &mut settings) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(()) => return 1,
            }
            if arg == "--email" {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("error: option '--email <email>' argument missing");
                    return 1;
                };
                email = Some(value.clone());
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--email=") {
                email = Some(value.to_string());
            } else if arg == "--sso" {
                sso = true;
            } else if arg == "--console" {
                console = true;
            } else if arg == "--claudeai" {
                claudeai = true;
            } else if arg == "--gateway-json" {
                gateway_json = true;
            } else if arg == "--organization-gateway" {
                organization_gateway = true;
            } else if arg == "--ldr" {
                ldr = true;
            } else if arg.starts_with('-') {
                eprintln!("error: unknown option '{arg}'");
                return 1;
            }
        }
        index += 1;
    }

    let another_sign_in = console || claudeai || sso || email.is_some();
    if organization_gateway && (another_sign_in || gateway_json || ldr) {
        eprintln!("--organization-gateway can't be used with another sign-in option.");
        return 1;
    }
    if console && claudeai {
        eprintln!("Error: --console and --claudeai can't be used together.");
        return 1;
    }
    if gateway_json && another_sign_in {
        eprintln!(
            "Error: --gateway-json can't be used with --console, --claudeai, --sso, or --email."
        );
        return 1;
    }
    if let Err(code) = resolve_settings(settings) {
        return code;
    }
    eprintln!("{LOGIN_UNAVAILABLE}");
    1
}

struct AuthStatus {
    logged_in: bool,
    auth_method: &'static str,
    api_provider: &'static str,
    provider: ApiProvider,
    analytics_disabled: bool,
    projects_directory: String,
    config_directory: String,
    login_expires_at: Option<String>,
    forced_login_method: Option<String>,
    console_profile_sign_in_offered: bool,
    api_key_source: Option<&'static str>,
    api_key_source_value: ApiKeySource,
    api_key_for_text: Option<&'static str>,
    raw_token_source: AuthTokenSource,
    effective_token_source: AuthTokenSource,
    email: Option<String>,
    org_id: Option<String>,
    org_name: Option<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
    stored_claude_ai_login: bool,
}

impl AuthStatus {
    fn json(&self) -> String {
        let mut output = Map::new();
        output.insert("loggedIn".to_string(), Value::Bool(self.logged_in));
        output.insert(
            "authMethod".to_string(),
            Value::String(self.auth_method.to_string()),
        );
        output.insert(
            "apiProvider".to_string(),
            Value::String(self.api_provider.to_string()),
        );
        output.insert(
            "analyticsDisabled".to_string(),
            Value::Bool(self.analytics_disabled),
        );
        output.insert(
            "projectsDirectory".to_string(),
            Value::String(self.projects_directory.clone()),
        );
        output.insert(
            "configDirectory".to_string(),
            Value::String(self.config_directory.clone()),
        );
        output.insert(
            "loginExpiresAt".to_string(),
            self.login_expires_at
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        if let Some(method) = &self.forced_login_method {
            output.insert(
                "forcedLoginMethod".to_string(),
                Value::String(method.clone()),
            );
        }
        if self.console_profile_sign_in_offered {
            output.insert("consoleProfileSignInOffered".to_string(), Value::Bool(true));
        }
        if let Some(source) = self.api_key_source {
            output.insert(
                "apiKeySource".to_string(),
                Value::String(source.to_string()),
            );
        }
        if self.auth_method == "claude.ai" {
            output.insert(
                "email".to_string(),
                self.email.clone().map(Value::String).unwrap_or(Value::Null),
            );
            output.insert(
                "orgId".to_string(),
                self.org_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            output.insert(
                "orgName".to_string(),
                self.org_name
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            output.insert(
                "subscriptionType".to_string(),
                self.subscription_type
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            output.insert("storedClaudeAiLogin".to_string(), Value::Bool(true));
            output.insert(
                "planTier".to_string(),
                Value::String(
                    plan_tier(
                        self.subscription_type.as_deref(),
                        self.rate_limit_tier.as_deref(),
                    )
                    .to_string(),
                ),
            );
        } else if self.auth_method == "api_key"
            && self.api_key_source_value == ApiKeySource::LoginManagedKey
        {
            output.insert(
                "email".to_string(),
                self.email.clone().map(Value::String).unwrap_or(Value::Null),
            );
            output.insert(
                "orgId".to_string(),
                self.org_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            output.insert(
                "orgName".to_string(),
                self.org_name
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
        }
        serde_json::to_string_pretty(&Value::Object(output)).unwrap_or_default()
    }

    fn text(&self) -> String {
        let mut lines = Vec::new();
        match self.provider {
            ApiProvider::Bedrock => {
                lines.push("API provider: Amazon Bedrock".to_string());
                let region = crate::utils::process_env::var("AWS_REGION")
                    .truthy()
                    .or_else(|| crate::utils::process_env::var("AWS_DEFAULT_REGION").truthy());
                lines.push(match region {
                    Some(region) => format!("AWS region: {region}"),
                    None => "AWS region: us-east-1 (default — set AWS_REGION or add a region to your AWS config)"
                        .to_string(),
                });
                return lines.join("\n");
            }
            ApiProvider::Vertex => {
                lines.push("API provider: Google Vertex AI".to_string());
                let region = crate::utils::process_env::var("CLOUD_ML_REGION")
                    .truthy()
                    .unwrap_or_else(|| "us-east5".to_string());
                lines.push(format!("Default region: {region}"));
                return lines.join("\n");
            }
            ApiProvider::Foundry => {
                lines.push("API provider: Microsoft Foundry".to_string());
                return lines.join("\n");
            }
            ApiProvider::FirstParty => {}
        }

        let subscription_in_use = self.auth_method == "claude.ai" && self.stored_claude_ai_login;
        if subscription_in_use {
            lines.push(format!(
                "Login method: {} account",
                claude_ai_subscription_name(self.subscription_type.as_deref())
            ));
        } else if self.raw_token_source != AuthTokenSource::None {
            let label = self.raw_token_source.status_label().unwrap_or("none");
            if self.effective_token_source == AuthTokenSource::None
                && self.api_key_source_value != ApiKeySource::None
            {
                lines.push(format!("Auth token: {label} · not in use"));
            } else if self.effective_token_source != AuthTokenSource::None {
                lines.push(format!("Auth token: {label}"));
            }
        }
        if let Some(source) = self.api_key_for_text {
            lines.push(format!("API key: {source}"));
        }

        if subscription_in_use {
            if let Some(organization) = self.org_name.as_deref().filter(|value| !value.is_empty()) {
                lines.push(format!("Organization: {organization}"));
            }
            if let Some(email) = self.email.as_deref().filter(|value| !value.is_empty()) {
                lines.push(format!("Email: {email}"));
            }
            if let Some(expires_at) = self.login_expires_at.as_deref() {
                if let Some(line) = sign_in_end_line(expires_at) {
                    lines.push(line);
                }
            }
        }

        if let Some(base_url) = crate::utils::process_env::var("ANTHROPIC_BASE_URL").truthy()
            && !crate::utils::model::providers::is_first_party_anthropic_base_url()
        {
            lines.push(format!("Anthropic base URL: {base_url}"));
        }

        if !self.logged_in && self.provider == ApiProvider::FirstParty {
            lines.push("Not logged in. Run claude auth login to authenticate.".to_string());
        }
        if lines.is_empty() {
            lines.push("Not logged in. Run claude auth login to authenticate.".to_string());
        }
        lines.join("\n")
    }
}

fn collect_status() -> AuthStatus {
    let provider = get_api_provider();
    let third_party = provider != ApiProvider::FirstParty;
    let token_status = get_auth_token_source();
    let raw_token_source = token_status.source;
    let api_key = get_anthropic_api_key_with_source(GetAnthropicApiKeyOptions {
        skip_retrieving_key_from_api_key_helper: true,
    });
    let env_api_key = crate::utils::process_env::var("ANTHROPIC_API_KEY")
        .truthy()
        .is_some()
        && !is_running_on_homespace();
    let mut api_key_source_value = api_key.source;
    if api_key_source_value == ApiKeySource::None && env_api_key {
        api_key_source_value = ApiKeySource::AnthropicApiKey;
    }
    let api_key_in_use = api_key_source_value != ApiKeySource::None && !third_party;
    let oauth_like = matches!(
        raw_token_source,
        AuthTokenSource::ClaudeAi
            | AuthTokenSource::ClaudeCodeOauthToken
            | AuthTokenSource::ClaudeCodeOauthTokenFileDescriptor
            | AuthTokenSource::CcrOauthTokenFile
    );
    let effective_token_source = if api_key_in_use && oauth_like {
        AuthTokenSource::None
    } else {
        raw_token_source
    };
    let auth_method = if third_party {
        "third_party"
    } else if effective_token_source == AuthTokenSource::ClaudeAi {
        "claude.ai"
    } else if effective_token_source == AuthTokenSource::ApiKeyHelper {
        "api_key_helper"
    } else if effective_token_source != AuthTokenSource::None {
        "oauth_token"
    } else if matches!(
        api_key_source_value,
        ApiKeySource::AnthropicApiKey | ApiKeySource::LoginManagedKey
    ) || env_api_key
    {
        "api_key"
    } else if api_key_source_value == ApiKeySource::ApiKeyHelper {
        "api_key_helper"
    } else {
        "none"
    };
    let logged_in = third_party
        || token_status.has_token
        || api_key_source_value != ApiKeySource::None
        || env_api_key;

    let credentials = crate::utils::secure_storage::get_secure_storage().read();
    let oauth = credentials
        .as_ref()
        .and_then(|value| value.get("claudeAiOauth"));
    let subscription_type = oauth
        .and_then(|oauth| oauth.get("subscriptionType"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let rate_limit_tier = oauth
        .and_then(|oauth| oauth.get("rateLimitTier"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let login_expires_at = if auth_method == "claude.ai" {
        oauth.and_then(credential_login_expiry)
    } else {
        None
    };

    let settings = crate::utils::settings::get_settings_with_errors().settings;
    let forced_login_method = settings
        .force_login_method
        .filter(|value| !value.is_empty());
    let console_profile_sign_in_offered = !third_party
        && forced_login_method.as_deref() != Some("claudeai")
        && settings.force_login_org_uuid.is_none();

    let account = if auth_method == "claude.ai"
        || (auth_method == "api_key" && api_key_source_value == ApiKeySource::LoginManagedKey)
    {
        crate::utils::config::load_global_config().oauth_account
    } else {
        None
    };
    let email = account
        .as_ref()
        .and_then(|account| account.email_address.clone())
        .filter(|value| !value.is_empty());
    let org_id = account
        .as_ref()
        .and_then(|account| account.organization_uuid.clone())
        .filter(|value| !value.is_empty());
    let org_name = account
        .as_ref()
        .and_then(|account| account.organization_name.clone())
        .filter(|value| !value.is_empty());

    let api_key_for_text = if api_key.key.is_some() {
        api_key.source.status_label()
    } else if env_api_key {
        Some("ANTHROPIC_API_KEY")
    } else {
        None
    };

    AuthStatus {
        logged_in,
        auth_method,
        api_provider: api_provider_label(provider),
        provider,
        analytics_disabled: is_analytics_disabled(),
        projects_directory: get_projects_dir().display().to_string(),
        config_directory: get_claude_config_home_dir().display().to_string(),
        login_expires_at,
        forced_login_method,
        console_profile_sign_in_offered,
        api_key_source: api_key_source_value.status_label(),
        api_key_source_value,
        api_key_for_text,
        raw_token_source,
        effective_token_source,
        email,
        org_id,
        org_name,
        subscription_type,
        rate_limit_tier,
        stored_claude_ai_login: auth_method == "claude.ai",
    }
}

fn api_provider_label(provider: ApiProvider) -> &'static str {
    match provider {
        ApiProvider::FirstParty => "firstParty",
        ApiProvider::Bedrock => "bedrock",
        ApiProvider::Vertex => "vertex",
        ApiProvider::Foundry => "foundry",
    }
}

fn plan_tier(subscription_type: Option<&str>, rate_limit_tier: Option<&str>) -> &'static str {
    match subscription_type {
        Some("pro") => "pro",
        Some("max") => match rate_limit_tier {
            Some("default_claude_max_5x") => "max_5x",
            Some("default_claude_max_20x") => "max_20x",
            _ => "max_other",
        },
        _ => "other",
    }
}

fn credential_login_expiry(oauth: &Value) -> Option<String> {
    oauth
        .get("refreshToken")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())?;
    let refresh_expires_at = oauth.get("refreshTokenExpiresAt")?.as_i64()?;
    if refresh_expires_at <= 0 {
        return None;
    }
    if let Some(expires_at) = oauth.get("expiresAt").and_then(Value::as_i64)
        && expires_at > refresh_expires_at + 3 * 86_400_000
    {
        return None;
    }
    let timestamp = chrono::DateTime::<Utc>::from_timestamp_millis(refresh_expires_at)?;
    Some(timestamp.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn sign_in_end_line(expires_at: &str) -> Option<String> {
    let expires_at = chrono::DateTime::parse_from_rfc3339(expires_at)
        .ok()?
        .with_timezone(&Utc);
    let now = Utc::now();
    let expires_ms = expires_at.timestamp_millis();
    let now_ms = now.timestamp_millis();
    let earliest = chrono::DateTime::<Utc>::from_timestamp_millis(expires_ms - 3 * 86_400_000)?;
    let earliest_date = earliest.format("%Y-%m-%d");
    let latest_date = expires_at.format("%Y-%m-%d");
    let days_to_earliest =
        earliest.timestamp_millis().div_euclid(86_400_000) - now_ms.div_euclid(86_400_000);
    let prefix = if expires_ms <= now_ms {
        format!("Sign-in may have ended: between {earliest_date} and {latest_date}")
    } else if days_to_earliest < 1 {
        format!("Sign-in ends: any day now, by {latest_date}")
    } else {
        format!("Sign-in ends: between {earliest_date} and {latest_date}")
    };
    Some(format!(
        "{prefix} ({})",
        expires_at.to_rfc3339_opts(SecondsFormat::Millis, true)
    ))
}
