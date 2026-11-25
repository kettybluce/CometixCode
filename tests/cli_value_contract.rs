//! Real executable coverage for 2.1.332 argument coercion. No model/network I/O.

use std::process::{Command, Output, Stdio};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cometix"))
        .args(args)
        .stdin(Stdio::null())
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .output()
        .expect("launch cometix")
}

#[test]
fn boolean_aliases_and_large_compact_windows_accept_version() {
    for option in [
        "--prompt-suggestions=yes",
        "--prompt-suggestions=off",
        "--prompt-suggestions=TRUE",
        "--prompt-suggestions=0",
        "--autocompact=2M",
        "--autocompact=100.5k",
        "--autocompact=1000001",
    ] {
        let output = run(&[option, "--version"]);
        assert!(output.status.success(), "{option}: {:?}", output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("(Claude Code)"));
        assert!(output.stderr.is_empty(), "{option}: {:?}", output);
    }
}

#[test]
fn malformed_values_fail_before_version_without_starting_tui() {
    for option in [
        "--prompt-suggestions=bogus",
        "--prompt-suggestions=",
        "--autocompact=99k",
        "--autocompact=1001",
        "--autocompact=9007199254740992",
        "--permission-prompts=HOST",
    ] {
        let output = run(&[option, "--version"]);
        assert_eq!(output.status.code(), Some(1), "{option}: {:?}", output);
        assert!(output.stdout.is_empty(), "{option}: {:?}", output);
        assert!(String::from_utf8_lossy(&output.stderr).contains("is invalid"));
    }
}

#[test]
fn required_options_without_arguments_fail_instead_of_launching_tui() {
    for option in [
        "--advisor",
        "--autocompact",
        "--permission-prompts",
        "--betas",
        "--name",
        "-n",
        "--model",
    ] {
        let output = run(&[option]);
        assert_eq!(output.status.code(), Some(1), "{option}: {:?}", output);
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("argument missing"));
    }
}

#[test]
fn version_short_circuits_later_invalid_values() {
    let output = run(&["--version", "--prompt-suggestions=bogus"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty());
}
