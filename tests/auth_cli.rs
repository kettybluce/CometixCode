//! Real-binary tests for the shipped Claude Code 2.1.332 `auth` surface.
//!
//! `status` is pinned byte-for-byte where the output is deterministic.
//! `logout` pins the local deletion and the offline "revocation unconfirmed"
//! message. `login` pins help/argument validation; its browser flow remains
//! closed by the product credential-write gate.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ROOT_HELP: &str = "Usage: cometix auth [options] [command]

Manage authentication

Options:
  -h, --help        Display help for command

Commands:
  help [command]    display help for command
  login [options]   Sign in to your Anthropic account
  logout [options]  Log out from your Anthropic account
  status [options]  Show authentication status
";

const LOGIN_HELP: &str = "Usage: cometix auth login [options]

Sign in to your Anthropic account

Options:
  --claudeai       Use Claude subscription (default)
  --console        Use Anthropic Console (API usage billing) instead of Claude
                   subscription
  --email <email>  Fill in the email address on the login page
  -h, --help       Display help for command
  --sso            Log in with SSO
";

const LOGOUT_HELP: &str = "Usage: cometix auth logout [options]

Log out from your Anthropic account

Options:
  -h, --help  Display help for command
";

const STATUS_HELP: &str = "Usage: cometix auth status [options]

Show authentication status

Options:
  -h, --help  Display help for command
  --json      Output as JSON (default)
  --text      Output as plain text
";

struct Fixture {
    root: PathBuf,
}

fn stored_oauth(with_subscription: bool) -> Value {
    let mut oauth = serde_json::Map::new();
    oauth.insert(
        "accessToken".to_string(),
        Value::String("test-access-token".to_string()),
    );
    oauth.insert(
        "refreshToken".to_string(),
        Value::String("test-refresh-token".to_string()),
    );
    oauth.insert(
        "scopes".to_string(),
        serde_json::json!(["user:inference", "user:profile"]),
    );
    if with_subscription {
        let future_ms = chrono::DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        oauth.insert("expiresAt".to_string(), future_ms.into());
        oauth.insert("refreshTokenExpiresAt".to_string(), future_ms.into());
        oauth.insert(
            "subscriptionType".to_string(),
            Value::String("pro".to_string()),
        );
        oauth.insert(
            "rateLimitTier".to_string(),
            Value::String("default_claude_max_20x".to_string()),
        );
    }
    let mut root = serde_json::Map::new();
    root.insert(format!("{}{}", "claudeAi", "Oauth"), Value::Object(oauth));
    Value::Object(root)
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cometix-auth-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("cwd")).unwrap();
        Self { root }
    }

    fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cometix"));
        command
            .args(args)
            .current_dir(self.root.join("cwd"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("CLAUDE_CONFIG_DIR", self.root.join("config"))
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .env("DISABLE_TELEMETRY", "1")
            .env("DISABLE_AUTOUPDATER", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, value) in envs {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn write_oauth(&self, value: Value) {
        fs::write(
            self.config().join(".credentials.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    fn write_account(&self, value: Value) {
        fs::write(
            self.config().join(".claude.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_stdout(output: &Output, expected: &str) {
    assert_eq!(stdout(output), expected, "stderr={}", stderr(output));
}

#[test]
fn help_and_root_errors_match_upstream() {
    let fixture = Fixture::new("help");
    for args in [
        vec!["auth", "--help"],
        vec!["auth", "-h"],
        vec!["auth", "help"],
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(0), "args={args:?}");
        assert_stdout(&output, ROOT_HELP);
        assert!(stderr(&output).is_empty());
    }

    let bare = fixture.run(&["auth"]);
    assert_eq!(bare.status.code(), Some(1));
    assert!(stdout(&bare).is_empty());
    assert_eq!(stderr(&bare), ROOT_HELP);

    let unknown = fixture.run(&["auth", "foo"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(stdout(&unknown).is_empty());
    assert_eq!(stderr(&unknown), "error: unknown command 'foo'\n");

    let help_unknown = fixture.run(&["auth", "help", "nope"]);
    assert_eq!(help_unknown.status.code(), Some(1));
    assert!(stdout(&help_unknown).is_empty());
    assert_eq!(stderr(&help_unknown), ROOT_HELP);
}

#[test]
fn subcommand_help_wins_over_unknown_options() {
    let fixture = Fixture::new("help-order");
    for (args, expected) in [
        (vec!["auth", "status", "--nope", "--help"], STATUS_HELP),
        (vec!["auth", "logout", "--nope", "-h"], LOGOUT_HELP),
        (vec!["auth", "login", "--nope", "--help"], LOGIN_HELP),
        (vec!["auth", "help", "status", "extra"], STATUS_HELP),
        (vec!["auth", "help", "login", "extra"], LOGIN_HELP),
        (vec!["auth", "help", "logout", "extra"], LOGOUT_HELP),
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(0), "args={args:?}");
        assert_stdout(&output, expected);
        assert!(stderr(&output).is_empty(), "args={args:?}");
    }

    let separator = fixture.run(&["auth", "status", "--", "--help"]);
    assert_eq!(separator.status.code(), Some(1));
    assert!(stdout(&separator).starts_with("{\n"));
    assert!(stderr(&separator).is_empty());
}

#[test]
fn status_without_credentials_matches_upstream_json() {
    let fixture = Fixture::new("status-empty");
    let output = fixture.run(&["auth", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).is_empty());
    assert_eq!(
        stdout(&output).replace(&fixture.root.display().to_string(), "<ROOT>"),
        r#"{
  "loggedIn": false,
  "authMethod": "none",
  "apiProvider": "firstParty",
  "analyticsDisabled": true,
  "projectsDirectory": "<ROOT>/config/projects",
  "configDirectory": "<ROOT>/config",
  "loginExpiresAt": null,
  "consoleProfileSignInOffered": true
}
"#
    );

    let text = fixture.run(&["auth", "status", "--text"]);
    assert_eq!(text.status.code(), Some(1));
    assert_stdout(
        &text,
        "Not logged in. Run claude auth login to authenticate.\n",
    );
}

#[test]
fn status_reports_api_key_and_oauth_sources() {
    let fixture = Fixture::new("status-sources");

    let api_key = fixture.run_with_env(&["auth", "status"], &[("ANTHROPIC_API_KEY", "test-key")]);
    assert_eq!(
        api_key.status.code(),
        Some(0),
        "stderr={}",
        stderr(&api_key)
    );
    let api_key_json: Value = serde_json::from_slice(&api_key.stdout).unwrap();
    assert_eq!(api_key_json["loggedIn"], true);
    assert_eq!(api_key_json["authMethod"], "api_key");
    assert_eq!(api_key_json["apiKeySource"], "ANTHROPIC_API_KEY");

    let api_key_text = fixture.run_with_env(
        &["auth", "status", "--text"],
        &[("ANTHROPIC_API_KEY", "test-key")],
    );
    assert_stdout(&api_key_text, "API key: ANTHROPIC_API_KEY\n");

    let auth_token = fixture.run_with_env(
        &["auth", "status", "--text"],
        &[("ANTHROPIC_AUTH_TOKEN", "test-token")],
    );
    assert_stdout(&auth_token, "Auth token: ANTHROPIC_AUTH_TOKEN\n");

    let oauth_env = fixture.run_with_env(
        &["auth", "status", "--text"],
        &[("CLAUDE_CODE_OAUTH_TOKEN", "env-token")],
    );
    assert_stdout(&oauth_env, "Auth token: CLAUDE_CODE_OAUTH_TOKEN\n");
}

#[test]
fn status_reports_stored_claude_ai_login_and_expiry() {
    let fixture = Fixture::new("status-oauth");
    fixture.write_oauth(stored_oauth(true));
    fixture.write_account(serde_json::json!({
        "oauthAccount": {
            "emailAddress": "a@b.c",
            "organizationUuid": "org-1",
            "organizationName": "Org Name"
        }
    }));

    let output = fixture.run(&["auth", "status"]);
    assert_eq!(output.status.code(), Some(0), "stderr={}", stderr(&output));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["loggedIn"], true);
    assert_eq!(json["authMethod"], "claude.ai");
    assert_eq!(json["loginExpiresAt"], "2030-01-01T00:00:00.000Z");
    assert_eq!(json["email"], "a@b.c");
    assert_eq!(json["orgId"], "org-1");
    assert_eq!(json["orgName"], "Org Name");
    assert_eq!(json["subscriptionType"], "pro");
    assert_eq!(json["storedClaudeAiLogin"], true);
    assert_eq!(json["planTier"], "pro");

    let text = fixture.run(&["auth", "status", "--text"]);
    assert_stdout(
        &text,
        "Login method: Claude Pro account\nOrganization: Org Name\nEmail: a@b.c\nSign-in ends: between 2029-12-29 and 2030-01-01 (2030-01-01T00:00:00.000Z)\n",
    );
}

#[test]
fn status_reports_third_party_providers_and_helper_without_executing_it() {
    let fixture = Fixture::new("status-provider");
    for (key, expected) in [
        (
            "CLAUDE_CODE_USE_BEDROCK",
            "API provider: Amazon Bedrock\nAWS region: us-east-1 (default — set AWS_REGION or add a region to your AWS config)",
        ),
        (
            "CLAUDE_CODE_USE_VERTEX",
            "API provider: Google Vertex AI\nDefault region: us-east5",
        ),
        ("CLAUDE_CODE_USE_FOUNDRY", "API provider: Microsoft Foundry"),
    ] {
        let output = fixture.run_with_env(&["auth", "status", "--text"], &[(key, "1")]);
        assert_eq!(output.status.code(), Some(0), "provider={key}");
        assert_stdout(&output, &format!("{expected}\n"));
    }

    let marker = fixture.root.join("helper-ran");
    fs::write(
        fixture.config().join("settings.json"),
        serde_json::json!({
            "apiKeyHelper": format!("touch {}; echo test-key", marker.display())
        })
        .to_string(),
    )
    .unwrap();
    let helper = fixture.run(&["auth", "status", "--text"]);
    assert_eq!(helper.status.code(), Some(0));
    assert_stdout(&helper, "Auth token: apiKeyHelper\n");
    assert!(
        !Path::new(&marker).exists(),
        "status must not run apiKeyHelper"
    );
}

#[test]
fn status_honors_forced_login_method_and_text_precedence() {
    let fixture = Fixture::new("status-force");
    fs::write(
        fixture.config().join("settings.json"),
        r#"{"forceLoginMethod":"claudeai"}"#,
    )
    .unwrap();
    let output = fixture.run(&["auth", "status"]);
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["forcedLoginMethod"], "claudeai");
    assert!(json.get("consoleProfileSignInOffered").is_none());

    let console = fixture.run(&["auth", "status", "--json", "--text"]);
    assert_eq!(console.status.code(), Some(1));
    assert_stdout(
        &console,
        "Not logged in. Run claude auth login to authenticate.\n",
    );
}

#[test]
fn unknown_options_and_settings_errors_match_upstream() {
    let fixture = Fixture::new("status-errors");
    for (args, expected) in [
        (
            vec!["auth", "status", "--nope"],
            "error: unknown option '--nope'\n",
        ),
        (
            vec!["auth", "status", "--json=true"],
            "error: unknown option '--json=true'\n",
        ),
        (
            vec!["auth", "status", "--settings"],
            "error: unknown option '--settings'\n",
        ),
        (
            vec!["auth", "--json", "status"],
            "error: unknown option '--json'\n",
        ),
        (
            vec!["auth", "login", "--email"],
            "error: option '--email <email>' argument missing\n",
        ),
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        assert!(stdout(&output).is_empty(), "args={args:?}");
        assert_eq!(stderr(&output), expected, "args={args:?}");
    }

    let missing = fixture.run(&["auth", "status", "--settings", "/tmp/no-such-auth.json"]);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(
        stderr(&missing),
        "Error: Settings file not found: /tmp/no-such-auth.json\n"
    );
}

#[test]
fn login_argument_validation_and_credential_gate_are_explicit() {
    let fixture = Fixture::new("login");
    for (args, expected) in [
        (
            vec!["auth", "login", "--console", "--claudeai"],
            "Error: --console and --claudeai can't be used together.\n",
        ),
        (
            vec!["auth", "login", "--gateway-json", "--console"],
            "Error: --gateway-json can't be used with --console, --claudeai, --sso, or --email.\n",
        ),
        (
            vec!["auth", "login", "--organization-gateway", "--console"],
            "--organization-gateway can't be used with another sign-in option.\n",
        ),
    ] {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        assert!(stdout(&output).is_empty(), "args={args:?}");
        assert_eq!(stderr(&output), expected, "args={args:?}");
    }

    let closed = fixture.run(&["auth", "login"]);
    assert_eq!(closed.status.code(), Some(1));
    assert!(stdout(&closed).is_empty());
    assert_eq!(
        stderr(&closed),
        "OAuth login is unavailable in this build (credential side effects are disabled).\n"
    );
    assert!(!fixture.config().join(".credentials.json").exists());
}

#[test]
fn logout_deletes_local_credentials_and_reports_unconfirmed_revocation() {
    let fixture = Fixture::new("logout");
    let credentials = fixture.config().join(".credentials.json");
    fixture.write_oauth(stored_oauth(false));

    let output = fixture.run(&["auth", "logout"]);
    assert_eq!(output.status.code(), Some(0), "stderr={}", stderr(&output));
    assert_stdout(
        &output,
        "Logged out on this computer, but Anthropic didn't confirm the login was revoked. It may still work until you revoke it at https://claude.ai/settings/claude-code\n",
    );
    assert!(!credentials.exists());

    let json = fixture.run(&["auth", "logout", "--json"]);
    assert_eq!(json.status.code(), Some(0));
    assert_stdout(
        &json,
        "{\"message\":\"Successfully logged out from your Anthropic account.\"}\n",
    );
}

#[test]
fn logout_without_credentials_is_idempotent() {
    let fixture = Fixture::new("logout-empty");
    let output = fixture.run(&["auth", "logout"]);
    assert_eq!(output.status.code(), Some(0));
    assert_stdout(
        &output,
        "Successfully logged out from your Anthropic account.\n",
    );
    let json = fixture.run(&["auth", "logout", "--json"]);
    assert_eq!(json.status.code(), Some(0));
    assert_stdout(
        &json,
        "{\"message\":\"Successfully logged out from your Anthropic account.\"}\n",
    );
}
