//! Real-binary tests for the shipped Claude Code 2.1.332 `import` contract.
//!
//! The target build advertises help but refuses to run the importer. These
//! tests pin that exact envelope instead of pretending an importer exists.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const HELP: &str = "Usage: cometix import [options] [source]

Import config from another AI coding agent into Claude Code

Arguments:
  source      Which agent to import from (codex, gemini, cursor)

Options:
  --dry-run   Show what would be imported without writing anything
  -h, --help  Display help for command
  --yes       Skip the interactive picker. In non-interactive mode, pass
              --yes=<digest> from the `/import` preview.
";

const UNAVAILABLE: &str = "`cometix import` is not yet available in this build. Run `cometix` and use /mcp or edit ~/.claude/settings.json directly.\n";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cometix-import-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("cwd")).unwrap();
        Self { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cometix"))
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
            .output()
            .unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_help(fixture: &Fixture, args: &[&str]) {
    let output = fixture.run(args);
    assert_eq!(output.status.code(), Some(0), "args={args:?}");
    assert_eq!(stdout(&output), HELP, "args={args:?}");
    assert!(stderr(&output).is_empty(), "args={args:?}");
}

fn assert_unavailable(fixture: &Fixture, args: &[&str]) {
    let output = fixture.run(args);
    assert_eq!(output.status.code(), Some(1), "args={args:?}");
    assert!(stdout(&output).is_empty(), "args={args:?}");
    assert_eq!(stderr(&output), UNAVAILABLE, "args={args:?}");
}

#[test]
fn help_matches_upstream_and_wins_over_unknown_options() {
    let fixture = Fixture::new("help");
    for args in [
        vec!["import", "--help"],
        vec!["import", "-h"],
        vec!["import", "--dry-run", "--help"],
        vec!["import", "codex", "-h"],
        vec!["import", "--bogus", "--help"],
        vec!["import", "--bogus", "--", "--help"],
    ] {
        assert_help(&fixture, &args);
    }
}

#[test]
fn separator_makes_later_help_a_positional_like_upstream() {
    let fixture = Fixture::new("separator");
    assert_unavailable(&fixture, &["import", "--", "--help"]);
    assert_unavailable(&fixture, &["import", "codex", "--", "-h"]);
}

#[test]
fn importer_reports_unavailable_for_sources_and_flags() {
    let fixture = Fixture::new("unavailable");
    for args in [
        vec!["import"],
        vec!["import", "codex"],
        vec!["import", "gemini"],
        vec!["import", "cursor"],
        vec!["import", "bogus"],
        vec!["import", "--dry-run"],
        vec!["import", "codex", "--dry-run"],
        vec!["import", "--yes"],
        vec!["import", "--yes=digest"],
        vec!["import", "codex", "--yes", "digest"],
        vec!["import", "--", "codex"],
        vec!["import", "codex", "extra"],
    ] {
        assert_unavailable(&fixture, &args);
    }
}

#[test]
fn unknown_options_report_commander_style_errors() {
    let fixture = Fixture::new("unknown");
    let cases = [
        (
            vec!["import", "--bogus"],
            "error: unknown option '--bogus'\n",
        ),
        (
            vec!["import", "--dry-run=true"],
            "error: unknown option '--dry-run=true'\n",
        ),
        (vec!["import", "-z"], "error: unknown option '-z'\n"),
        (
            vec!["import", "codex", "--dry-run=true"],
            "error: unknown option '--dry-run=true'\n",
        ),
        (
            vec!["import", "--yes=digest", "--bogus"],
            "error: unknown option '--yes=digest'\n",
        ),
    ];
    for (args, expected) in cases {
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        assert!(stdout(&output).is_empty(), "args={args:?}");
        assert_eq!(stderr(&output), expected, "args={args:?}");
    }
}

#[test]
fn importer_does_not_write_settings() {
    let fixture = Fixture::new("no-write");
    let config_settings = fixture.root.join("config/settings.json");
    let home_settings = fixture.root.join("home/.claude/settings.json");
    let output = fixture.run(&["import", "codex", "--dry-run", "--yes=digest"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!config_settings.exists());
    assert!(!home_settings.exists());
}
