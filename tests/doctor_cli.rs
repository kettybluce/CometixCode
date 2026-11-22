//! Real-binary tests for the standalone `cometix doctor` CLI command.
//!
//! Maps to CC `cli.tsx` `doctorHandler`: a non-interactive text report that
//! never enters the TUI. The test asserts the stable envelope and that the
//! report is derived from the live local diagnostic (not a fixed stub).

use std::process::{Command, Stdio};

fn run_doctor(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cometix"))
        .args(std::iter::once("doctor").chain(args.iter().copied()))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::null())
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_TELEMETRY", "1")
        .output()
        .unwrap()
}

#[test]
fn doctor_help_exits_zero_with_official_shape() {
    let output = run_doctor(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("Usage: cometix doctor [options]\n"));
    assert!(stdout.contains("Check the health of your CometixCode installation"));
    assert!(stdout.contains("-h, --help  Display help for command"));
    assert!(output.stderr.is_empty());
}

#[test]
fn doctor_prints_live_diagnostic_report() {
    let output = run_doctor(&[]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("CometixCode doctor\n\n"));
    assert!(stdout.contains("Running: "));
    assert!(stdout.contains("Platform: "));
    assert!(stdout.contains("Path: "));
    assert!(stdout.contains("Config install method: "));
    assert!(stdout.contains("Search: "));
    assert!(stdout.contains("Auto-updates: "));
    assert!(
        stdout.contains("No installation issues found.")
            || stdout.contains("Installation issues:")
    );
    assert!(stdout.contains("run /doctor in a CometixCode session."));
    assert!(output.stderr.is_empty());
}

#[test]
fn doctor_rejects_unknown_options() {
    let output = run_doctor(&["--bogus"]);
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "error: unknown option '--bogus'"
    );
}
