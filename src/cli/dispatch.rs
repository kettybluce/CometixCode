//! Dispatch parsed [`CliConfig`] — Live continues; Unimplemented exits 1.

use super::config::CliConfig;
use crate::constants::product::VERSION;

/// Apply Live side effects that must run before interactive launch (bare env).
pub fn apply_live_env_side_effects(config: &CliConfig) {
    if config.bare {
        crate::utils::process_env::set("CLAUDE_CODE_SIMPLE", "1");
    }
    // `--include-hook-events` enables every known hook lifecycle event for the
    // stream-json output. The handler registration/output path is owned by
    // `utils/hooks/hook_events`.
    if config.include_hook_events {
        crate::utils::hooks::hook_events::set_all_hook_events_enabled(true);
    }
    // `--ax-screen-reader` maps to CC's accessibility mode: flat text, no
    // decorative borders or animations. The existing render path reads
    // `CLAUDE_CODE_ACCESSIBILITY` (TextInput cursor gate, etc.).
    if config.ax_screen_reader {
        crate::utils::process_env::set("CLAUDE_CODE_ACCESSIBILITY", "1");
    }
    // `--enable-auto-mode` is the persistent-consent CLI opt-in. Set the flag
    // that `has_auto_mode_opt_in_any_source()` / permission setup read; the
    // feature gate itself still decides whether auto mode is actually usable.
    if config.enable_auto_mode {
        crate::utils::permissions::auto_mode_state::set_auto_mode_flag_cli(true);
    }
    // `--prompt-suggestions [value]` maps to the same
    // `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` env channel that
    // `should_enable_prompt_suggestion_with_snapshots` reads (truthy wins,
    // defined-falsy disables).
    if let Some(value) = &config.prompt_suggestions {
        crate::utils::process_env::set("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION", value);
    }
    // `--brief` maps to CC's `maybeActivateBrief()` at startup; set the opt-in
    // flag directly because USER_MSG_OPT_IN is seeded once from the env var.
    if config.brief {
        crate::utils::process_env::set("CLAUDE_CODE_BRIEF", "1");
        crate::bootstrap::state::set_user_msg_opt_in(true);
    }
    // `--betas` feeds the same `ANTHROPIC_BETAS` channel that
    // `utils/betas.rs` reads for every model-derived beta header. Joining with
    // commas matches CC's comma-separated beta header format.
    if !config.betas.is_empty() {
        crate::utils::process_env::set("ANTHROPIC_BETAS", &config.betas.join(","));
        crate::utils::betas::clear_betas_caches();
    }
    // `--autocompact` writes the parsed window to the env channel read by
    // `get_effective_context_window_size`; `auto` clears any override.
    if let Some(raw) = &config.autocompact
        && let Ok(Some(tokens)) =
            crate::services::compact::auto_compact::parse_autocompact_value(raw)
    {
        crate::utils::process_env::set(
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            &tokens.to_string(),
        );
    }
}

/// Validate options that must hard-fail before any launch path (matching
/// commander's argument coercion). Returns an error message on invalid input.
fn validate_cli_values(config: &CliConfig) -> Result<(), String> {
    if let Some(raw) = &config.autocompact {
        crate::services::compact::auto_compact::parse_autocompact_value(raw)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Short-circuit Unimplemented reasons to stderr; returns exit code `1`.
pub fn exit_unimplemented(reasons: &[String]) -> i32 {
    if reasons.is_empty() {
        eprintln!("Unimplemented: unknown");
    } else if reasons.len() == 1 {
        eprintln!("Unimplemented: {}", reasons[0]);
    } else {
        eprintln!("Unimplemented: {}", reasons.join(", "));
    }
    1
}

/// Maps to: CC `entrypoints/cli.tsx` + `main.tsx` early dispatch after parse.
///
/// Returns `Some(exit_code)` when the process should exit without interactive
/// [`crate::main::run`]. Returns `None` to continue into interactive launch.
pub fn dispatch(config: &CliConfig) -> Option<i32> {
    if let Err(message) = validate_cli_values(config) {
        eprintln!("{message}");
        return Some(1);
    }
    apply_live_env_side_effects(config);

    if config.show_version {
        println!("{VERSION} (Claude Code)");
        return Some(0);
    }

    if config.show_help {
        CliConfig::print_help();
        return Some(0);
    }

    // Maps to CC `main.tsx` validation: this negated option is print-only.
    if config.session_persistence == Some(false) && !config.print {
        eprintln!("Error: --no-session-persistence can only be used with --print mode.");
        return Some(1);
    }

    // Non-interactive MCP configuration commands own their stdin/stdout and
    // must run before the generic subcommand-unimplemented guard below.
    if config.is_live_mcp_command() {
        let reasons = config.unimplemented_reasons();
        if !reasons.is_empty() {
            return Some(exit_unimplemented(&reasons));
        }
        return crate::commands::mcp::maybe_run_mcp_cli(&config.argv);
    }

    // Local diagnostics own their stdout and must run before the generic
    // subcommand-unimplemented guard. CC's `doctorHandler` is likewise a
    // standalone CLI path that never opens the interactive TUI.
    if config.subcommand.as_deref() == Some("doctor") {
        return crate::commands::doctor::maybe_run_doctor_cli(&config.argv);
    }

    // Preserve historical ant-only / feature-gated messaging where useful,
    // but unify on `Unimplemented:` for incomplete commander surface.
    let reasons = config.unimplemented_reasons();
    if !reasons.is_empty() {
        return Some(exit_unimplemented(&reasons));
    }

    // Maps to CC `cli/print.ts`: print/SDK mode owns the query lifetime and
    // structured stdout protocol instead of entering the retained TUI.
    if config.print {
        return Some(crate::cli::print::run(config));
    }

    // Interactive prompt-only without -p is Partial: allowed (typed later in REPL).
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse::parse_cli_config;
    use crate::utils::test_env::{EnvVarGuard, TEST_ENV_LOCK};

    fn argv(args: &[&str]) -> Vec<String> {
        std::iter::once("cometix".to_string())
            .chain(args.iter().map(|a| (*a).to_string()))
            .collect()
    }

    #[test]
    fn dispatch_version_exits_zero() {
        let c = parse_cli_config(&argv(&["-v"]));
        assert_eq!(dispatch(&c), Some(0));
    }

    #[test]
    fn dispatch_rejects_no_session_persistence_outside_print_mode() {
        let config = parse_cli_config(&argv(&["--no-session-persistence"]));
        assert_eq!(dispatch(&config), Some(1));
    }

    #[test]
    fn dispatch_interactive_default_continues() {
        let c = parse_cli_config(&argv(&[]));
        assert_eq!(dispatch(&c), None);
    }

    #[test]
    fn dispatch_bare_sets_env_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _simple = EnvVarGuard::unset("CLAUDE_CODE_SIMPLE");
        let c = parse_cli_config(&argv(&["--bare"]));
        assert_eq!(dispatch(&c), None);
        assert!(crate::utils::env_utils::is_env_truthy(
            crate::utils::process_env::var("CLAUDE_CODE_SIMPLE").as_deref()
        ));
    }

    #[test]
    fn dispatch_autocompact_sets_env_and_rejects_invalid() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::unset("CLAUDE_CODE_AUTO_COMPACT_WINDOW");
        let c = parse_cli_config(&argv(&["--autocompact", "500k"]));
        assert_eq!(dispatch(&c), None);
        assert_eq!(
            crate::utils::process_env::var("CLAUDE_CODE_AUTO_COMPACT_WINDOW").as_deref(),
            Some("500000")
        );

        // Invalid values hard-fail before any launch path, even with --version.
        let bad = parse_cli_config(&argv(&["--autocompact", "bogus", "--version"]));
        assert_eq!(dispatch(&bad), Some(1));
    }

    #[test]
    fn dispatch_include_hook_events_enables_all_hook_events_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        crate::utils::hooks::hook_events::clear_hook_event_state();
        let c = parse_cli_config(&argv(&["--include-hook-events"]));
        assert_eq!(dispatch(&c), None);
        assert!(crate::utils::hooks::hook_events::should_emit_hook_event("PreToolUse"));
        crate::utils::hooks::hook_events::clear_hook_event_state();
    }

    #[test]
    fn dispatch_ax_screen_reader_sets_accessibility_env_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::unset("CLAUDE_CODE_ACCESSIBILITY");
        let c = parse_cli_config(&argv(&["--ax-screen-reader"]));
        assert_eq!(dispatch(&c), None);
        assert_eq!(
            crate::utils::process_env::var("CLAUDE_CODE_ACCESSIBILITY").as_deref(),
            Some("1")
        );
        assert!(crate::utils::env_utils::is_env_truthy(
            crate::utils::process_env::var("CLAUDE_CODE_ACCESSIBILITY").as_deref()
        ));
    }

    #[test]
    fn dispatch_enable_auto_mode_sets_cli_flag_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        crate::utils::permissions::auto_mode_state::set_auto_mode_flag_cli(false);
        let c = parse_cli_config(&argv(&["--enable-auto-mode"]));
        assert_eq!(dispatch(&c), None);
        assert!(crate::utils::permissions::auto_mode_state::get_auto_mode_flag_cli());
        assert!(crate::utils::permissions::permission_setup::has_auto_mode_opt_in_any_source());
        crate::utils::permissions::auto_mode_state::set_auto_mode_flag_cli(false);
    }

    #[test]
    fn dispatch_prompt_suggestions_sets_env_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::unset("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION");
        let c = parse_cli_config(&argv(&["--prompt-suggestions", "false"]));
        assert_eq!(dispatch(&c), None);
        assert_eq!(
            crate::utils::process_env::var("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION").as_deref(),
            Some("false")
        );

        let c = parse_cli_config(&argv(&["--prompt-suggestions"]));
        assert_eq!(dispatch(&c), None);
        assert_eq!(
            crate::utils::process_env::var("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn dispatch_brief_sets_user_msg_opt_in_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _brief = EnvVarGuard::unset("CLAUDE_CODE_BRIEF");
        // Reset before and after so this test does not leak into siblings.
        crate::bootstrap::state::set_user_msg_opt_in(false);
        let c = parse_cli_config(&argv(&["--brief"]));
        assert_eq!(dispatch(&c), None);
        assert!(crate::bootstrap::state::get_user_msg_opt_in());
        assert_eq!(
            crate::utils::process_env::var("CLAUDE_CODE_BRIEF").as_deref(),
            Some("1")
        );
        crate::bootstrap::state::set_user_msg_opt_in(false);
    }

    #[test]
    fn dispatch_betas_sets_anthropic_betas_and_continues() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _betas = EnvVarGuard::unset("ANTHROPIC_BETAS");
        let c = parse_cli_config(&argv(&["--betas", "beta-one", "beta-two"]));
        assert!(c.unimplemented_reasons().is_empty());
        assert_eq!(dispatch(&c), None);
        assert_eq!(
            crate::utils::process_env::var("ANTHROPIC_BETAS").as_deref(),
            Some("beta-one,beta-two")
        );
        assert!(crate::utils::betas::get_all_model_betas("claude-sonnet-4-6")
            .iter()
            .any(|beta| beta == "beta-one"));
    }

    #[test]
    fn dispatch_model_and_permission_continue_interactive() {
        let c = parse_cli_config(&argv(&[
            "--model",
            "sonnet",
            "--effort",
            "high",
            "--verbose",
            "--permission-mode",
            "acceptEdits",
            "--dangerously-skip-permissions",
            "--allowedTools",
            "Read",
        ]));
        assert!(c.unimplemented_reasons().is_empty());
        assert_eq!(dispatch(&c), None);
        assert_eq!(c.model.as_deref(), Some("sonnet"));
        assert_eq!(c.effort.as_deref(), Some("high"));
        assert!(c.verbose);
        assert_eq!(c.permission_mode.as_deref(), Some("acceptEdits"));
        assert!(c.dangerously_skip_permissions);
        assert_eq!(c.allowed_tools, vec!["Read".to_string()]);
    }

    #[test]
    fn dispatch_thinking_is_live() {
        let c = parse_cli_config(&argv(&["--thinking", "enabled"]));
        assert!(c.unimplemented_reasons().is_empty());
        assert_eq!(dispatch(&c), None);
        assert_eq!(c.thinking.as_deref(), Some("enabled"));
    }

    #[test]
    fn dispatch_max_thinking_tokens_is_live() {
        let c = parse_cli_config(&argv(&["--max-thinking-tokens", "8000"]));
        assert!(c.unimplemented_reasons().is_empty());
        assert_eq!(c.max_thinking_tokens.as_deref(), Some("8000"));
        assert_eq!(dispatch(&c), None);
    }
}
