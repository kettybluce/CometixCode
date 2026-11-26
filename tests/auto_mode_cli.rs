//! Real-binary tests for `cometix auto-mode`.
//!
//! The oracle is the frozen Claude Code 2.1.332 CLI: trusted-source merge,
//! `$defaults` expansion, label filtering, and the reset file rewrite.

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cometix-auto-mode-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("project")).unwrap();
        Self { root }
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("config/settings.json")
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn write_settings(&self, value: &Value) {
        fs::write(
            self.settings_path(),
            format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Output {
        self.run_with_prefix(args, stdin, &[])
    }

    fn run_with_prefix(&self, args: &[&str], stdin: Option<&str>, prefix: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cometix"));
        command
            .args(prefix)
            .args(std::iter::once("auto-mode").chain(args.iter().copied()))
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("CLAUDE_CONFIG_DIR", self.root.join("config"))
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .env("DISABLE_TELEMETRY", "1")
            .env("DISABLE_AUTOUPDATER", "1")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        if let Some(input) = stdin {
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn parsed_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON stdout: {error}\nstdout={}\nstderr={}",
            stdout(output),
            stderr(output)
        )
    })
}

fn counts(value: &Value) -> (usize, usize, usize, usize) {
    (
        value["allow"].as_array().unwrap().len(),
        value["soft_deny"].as_array().unwrap().len(),
        value["hard_deny"].as_array().unwrap().len(),
        value["environment"].as_array().unwrap().len(),
    )
}

fn read_settings(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn defaults_matches_frozen_official_rules_and_label_filter() {
    let fixture = Fixture::new("defaults");
    let output = fixture.run(&["defaults"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(counts(&parsed_stdout(&output)), (16, 72, 1, 21));
    assert_eq!(
        parsed_stdout(&output)
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["allow", "soft_deny", "hard_deny", "environment"]
    );

    let git = fixture.run(&["defaults", "--label", "git"], None);
    assert!(git.status.success());
    assert_eq!(counts(&parsed_stdout(&git)), (1, 1, 0, 0));
    assert!(
        parsed_stdout(&git)["soft_deny"][0]
            .as_str()
            .unwrap()
            .starts_with("Git Destructive")
    );

    let data = fixture.run(&["defaults", "--label=Data"], None);
    assert!(data.status.success());
    assert_eq!(counts(&parsed_stdout(&data)), (0, 0, 1, 1));

    let no_match = fixture.run(&["defaults", "--label", "zzz-no-match"], None);
    assert!(no_match.status.success());
    assert_eq!(counts(&parsed_stdout(&no_match)), (0, 0, 0, 0));
}

#[test]
fn config_uses_trusted_sources_defaults_and_sentinel_expansion() {
    let fixture = Fixture::new("config");
    let defaults = parsed_stdout(&fixture.run(&["defaults"], None));
    let empty = fixture.run(&["config"], None);
    assert!(empty.status.success());
    assert_eq!(parsed_stdout(&empty), defaults);

    fixture.write_settings(&json!({
        "theme": "dark",
        "autoMode": {
            "allow": ["$defaults", "custom-allow"],
            "soft_deny": ["custom-soft"],
            "hard_deny": ["$defaults", "custom-hard"],
            "environment": ["$defaults"]
        }
    }));
    let configured = fixture.run(&["config"], None);
    assert!(configured.status.success(), "{}", stderr(&configured));
    let value = parsed_stdout(&configured);
    assert_eq!(counts(&value), (17, 1, 2, 21));
    assert_eq!(value["allow"][16], "custom-allow");
    assert_eq!(value["soft_deny"][0], "custom-soft");
    assert_eq!(value["hard_deny"][1], "custom-hard");

    // Project and local settings are repo-controllable and must not affect the
    // classifier config.
    fs::create_dir_all(fixture.project().join(".claude")).unwrap();
    fs::write(
        fixture.project().join(".claude/settings.json"),
        r#"{"autoMode":{"allow":["project-allow"]}}"#,
    )
    .unwrap();
    fs::write(
        fixture.project().join(".claude/settings.local.json"),
        r#"{"autoMode":{"allow":["local-allow"]}}"#,
    )
    .unwrap();
    let still_trusted = parsed_stdout(&fixture.run(&["config"], None));
    assert_eq!(still_trusted["allow"][16], "custom-allow");
    assert!(
        !still_trusted["allow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule == "project-allow" || rule == "local-allow")
    );
}

#[test]
fn config_reads_the_settings_flag_source() {
    let fixture = Fixture::new("settings-flag");
    let flag = fixture.root.join("flag-settings.json");
    fs::write(
        &flag,
        r#"{"autoMode":{"allow":["flag-allow"],"hard_deny":["flag-hard"]}}"#,
    )
    .unwrap();
    let output =
        fixture.run_with_prefix(&["config"], None, &["--settings", flag.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = parsed_stdout(&output);
    assert_eq!(value["allow"], json!(["flag-allow"]));
    assert_eq!(value["hard_deny"], json!(["flag-hard"]));
}

#[test]
fn reset_removes_only_auto_mode_and_preserves_other_settings() {
    let fixture = Fixture::new("reset-yes");
    fixture.write_settings(&json!({
        "theme": "dark",
        "env": {"FOO": "bar"},
        "autoMode": {"allow": ["x"], "hard_deny": ["y"]}
    }));
    let output = fixture.run(&["reset", "-y"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("  - allow (1 entry)"));
    assert!(text.contains("  - hard_deny (1 entry)"));
    assert!(text.contains("autoMode section removed"));
    let written = read_settings(&fixture.settings_path());
    assert!(written.get("autoMode").is_none());
    assert_eq!(written["theme"], "dark");
    assert_eq!(written["env"]["FOO"], "bar");
    assert!(
        fs::read_to_string(fixture.settings_path())
            .unwrap()
            .ends_with('\n')
    );

    let config = parsed_stdout(&fixture.run(&["config"], None));
    assert_eq!(counts(&config), (16, 72, 1, 21));
}

#[test]
fn reset_without_section_is_a_noop() {
    let fixture = Fixture::new("reset-noop");
    fixture.write_settings(&json!({"theme": "light", "env": {"A": "B"}}));
    let before = fs::read(fixture.settings_path()).unwrap();
    let output = fixture.run(&["reset", "--yes"], None);
    assert!(output.status.success());
    assert!(stdout(&output).contains("already at defaults"));
    assert_eq!(fs::read(fixture.settings_path()).unwrap(), before);
}

#[test]
fn reset_confirmation_and_invalid_json_are_honest() {
    let fixture = Fixture::new("reset-confirm");
    fixture.write_settings(&json!({"theme": "dark", "autoMode": {"allow": ["x"]}}));
    let before = fs::read(fixture.settings_path()).unwrap();
    let declined = fixture.run(&["reset"], Some("n\n"));
    assert!(!declined.status.success());
    assert_eq!(declined.status.code(), Some(1));
    assert!(stderr(&declined).contains("Aborted."));
    assert_eq!(fs::read(fixture.settings_path()).unwrap(), before);

    fs::write(fixture.settings_path(), "{bad json").unwrap();
    let invalid = fixture.run(&["reset", "-y"], None);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(stderr(&invalid).contains("contains invalid JSON"));
    assert_eq!(
        fs::read_to_string(fixture.settings_path()).unwrap(),
        "{bad json"
    );

    fixture.write_settings(&json!({"autoMode": {"allow": ["x"]}}));
    let accepted = fixture.run(&["reset"], Some("yes\n"));
    assert!(accepted.status.success(), "{}", stderr(&accepted));
    assert!(
        read_settings(&fixture.settings_path())
            .get("autoMode")
            .is_none()
    );
}

#[test]
fn help_unknown_command_and_argument_errors_match_oracle_envelope() {
    let fixture = Fixture::new("errors");
    let help = fixture.run(&["--help"], None);
    assert!(help.status.success());
    assert!(stdout(&help).starts_with("Usage: cometix auto-mode [options] [command]"));
    assert!(stdout(&help).contains("hard_deny"));

    let missing = fixture.run(&[], None);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stdout(&missing).is_empty());
    assert!(stderr(&missing).contains("Usage: cometix auto-mode"));

    let unknown = fixture.run(&["bogus"], None);
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(stderr(&unknown).trim(), "error: unknown command 'bogus'");

    let missing_label = fixture.run(&["defaults", "--label"], None);
    assert_eq!(missing_label.status.code(), Some(1));
    assert_eq!(
        stderr(&missing_label).trim(),
        "error: option '--label <prefix>' argument missing"
    );

    let bad_option = fixture.run(&["defaults", "--bogus"], None);
    assert_eq!(bad_option.status.code(), Some(1));
    assert_eq!(
        stderr(&bad_option).trim(),
        "error: unknown option '--bogus'"
    );
}

#[test]
fn critique_without_custom_rules_does_not_call_a_model() {
    let fixture = Fixture::new("critique-empty");
    let output = fixture.run(&["critique"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("No custom auto mode rules found."));
    assert!(stdout(&output).contains("autoMode.{allow, soft_deny, hard_deny, environment}"));
}
