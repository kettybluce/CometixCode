//! Fast, pure parser coverage using the production files directly.
//!
//! This target can also be compiled with `rustc --edition=2024 --test` on a
//! resource-constrained development host. It does not replace the full nextest
//! gate or real-binary tests; it has no runtime/config/environment fixtures.

#[allow(dead_code)]
#[path = "../src/cli/config.rs"]
mod config;
#[path = "../src/cli/parse.rs"]
mod parse;
#[path = "../src/cli/value_parsers.rs"]
mod value_parsers;

#[test]
fn newly_added_value_options_do_not_accept_longer_flag_names() {
    for option in [
        "--advisor-extra",
        "--betas-extra",
        "--autocompact-window",
        "--permission-prompts-extra",
    ] {
        let argv = ["cometix", option, "ordinary prompt"].map(str::to_string);
        let parsed = parse::parse_cli_config(&argv);
        assert_eq!(parsed.unknown_flags, vec![option], "{option}");
        assert_eq!(
            parsed.prompt.as_deref(),
            Some("ordinary prompt"),
            "{option}"
        );
        assert!(parsed.advisor_model.is_none());
        assert!(parsed.betas.is_empty());
        assert!(parsed.autocompact.is_none());
        assert!(parsed.permission_prompts.is_none());
    }
}

#[test]
fn value_options_require_exact_names_before_consuming_arguments() {
    for option in [
        "--debug-file",
        "--output-format",
        "--input-format",
        "--json-schema",
        "--thinking",
        "--effort",
        "--max-thinking-tokens",
        "--max-turns",
        "--max-budget-usd",
        "--task-budget",
        "--fallback-model",
        "--workload",
        "--permission-mode",
        "--permission-prompt-tool",
        "--allowedTools",
        "--allowed-tools",
        "--disallowedTools",
        "--disallowed-tools",
        "--session-id",
        "--system-prompt-file",
        "--append-system-prompt-file",
        "--system-prompt",
        "--append-system-prompt",
        "--mcp-config",
        "--setting-sources",
        "--settings",
        "--add-dir",
        "--agents",
        "--agent-id",
        "--agent-name",
        "--team-name",
        "--agent-color",
        "--parent-session-id",
        "--teammate-mode",
        "--agent-type",
    ] {
        let invalid = format!("{option}-extra");
        let parsed = parse_args(&[&invalid, "ordinary prompt"]);
        assert_eq!(parsed.unknown_flags, [invalid.as_str()], "{option}");
        assert_eq!(
            parsed.prompt.as_deref(),
            Some("ordinary prompt"),
            "{option}"
        );
    }
}

#[test]
fn compact_window_matches_observed_numeric_prefix_contracts() {
    for value in ["100000junk", "100.5.2k", "100kk", "1e2junk"] {
        assert!(
            value_parsers::parse_autocompact_value(value).is_ok(),
            "{value}"
        );
    }
    for value in ["1,,00", "1,,000", "1__00", "1e", "1e+"] {
        assert!(
            value_parsers::parse_autocompact_value(value).is_err(),
            "{value}"
        );
    }
}

#[test]
fn compact_acceptance_matches_recorded_original_2_1_332_cases() {
    for row in include_str!("fixtures/cli/autocompact-2.1.332.tsv").lines() {
        if row.starts_with('#') {
            continue;
        }
        let (exit_code, raw) = row.split_once('\t').unwrap();
        assert_eq!(
            value_parsers::parse_autocompact_value(raw).is_ok(),
            exit_code == "0",
            "{raw:?}"
        );
    }
}

#[test]
fn common_required_options_report_missing_arguments() {
    for option in [
        "--debug-file",
        "--output-format",
        "--input-format",
        "--json-schema",
        "--effort",
        "--max-budget-usd",
        "--fallback-model",
        "--permission-mode",
        "--permission-prompt-tool",
        "--tools",
        "--allowedTools",
        "--allowed-tools",
        "--disallowedTools",
        "--disallowed-tools",
        "--session-id",
        "--system-prompt-file",
        "--append-system-prompt-file",
        "--system-prompt",
        "--append-system-prompt",
        "--mcp-config",
        "--setting-sources",
        "--settings",
        "--add-dir",
        "--agents",
        "--agent",
        "--plugin-dir",
    ] {
        assert!(
            parse_args(&[option])
                .validate_values()
                .unwrap_err()
                .contains("argument missing"),
            "{option}"
        );
    }
}

#[test]
fn mcp_serve_routes_to_a_noninteractive_command() {
    let parsed = parse_args(&["mcp", "serve"]);
    assert!(parsed.is_live_mcp_command());
    assert!(parsed.unimplemented_reasons().is_empty());
}

fn parse_args(args: &[&str]) -> config::CliConfig {
    let argv = std::iter::once("cometix")
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect::<Vec<_>>();
    parse::parse_cli_config(&argv)
}

#[test]
fn required_new_options_report_missing_arguments() {
    for (option, declaration) in [
        ("--advisor", "--advisor <model>"),
        ("--autocompact", "--autocompact <auto|tokens>"),
        ("--permission-prompts", "--permission-prompts <target>"),
        ("--betas", "--betas <betas...>"),
        ("--name", "-n, --name <name>"),
        ("-n", "-n, --name <name>"),
        ("--model", "--model <model>"),
    ] {
        assert_eq!(
            parse_args(&[option]).validate_values(),
            Err(format!("error: option '{declaration}' argument missing")),
            "{option}"
        );
    }
}

#[test]
fn suggestions_validate_aliases_and_preserve_prompt_after_end_of_options() {
    for value in [
        "true", "false", "1", "0", "yes", "no", "on", "off", "TRUE", "False",
    ] {
        let c = parse_args(&["--prompt-suggestions", value, "ordinary prompt"]);
        assert!(c.validate_values().is_ok(), "{value}");
        assert_eq!(c.prompt.as_deref(), Some("ordinary prompt"));
    }
    let c = parse_args(&["--prompt-suggestions", "--", "--ordinary prompt"]);
    assert_eq!(c.prompt_suggestions.as_deref(), Some("true"));
    assert_eq!(c.prompt.as_deref(), Some("--ordinary prompt"));
    assert!(c.unknown_flags.is_empty());
    for args in [
        vec!["--prompt-suggestions=bogus", "--version"],
        vec!["--prompt-suggestions", "bogus", "--version"],
        vec!["--prompt-suggestions=", "--version"],
    ] {
        assert!(parse_args(&args).validate_values().is_err());
    }
}

#[test]
fn option_coercion_and_version_respect_argument_order() {
    for (option, invalid, valid) in [
        ("--autocompact", "50k", "500k"),
        ("--permission-prompts", "bogus", "host"),
        ("--prompt-suggestions", "bogus", "yes"),
    ] {
        assert!(
            parse_args(&[option, invalid, option, valid, "--version"])
                .validate_values()
                .is_err()
        );
        let version = parse_args(&["--version", option, invalid]);
        assert!(version.show_version);
        assert!(version.validate_values().is_ok());
    }
    let c = parse_args(&["--betas", "--version"]);
    assert!(!c.show_version);
    assert_eq!(c.betas, ["--version"]);
}
