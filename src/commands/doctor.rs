//! Maps to: CC `commands/doctor/doctor.tsx`.
//!
//! The official local-jsx command `call(...)` returns `<Doctor onDone={onDone} />`
//! from `screens/Doctor.tsx`. Cometix keeps the same command-to-screen boundary:
//! command processing opens the local UI, while the screen implementation lives
//! in `src/screens/doctor.rs`.
//!
//! The non-interactive `cometix doctor` shell command reuses the very same
//! `utils::doctor_diagnostic` data model and prints a text report, matching
//! CC `cli.tsx`'s `doctorHandler` (which never opens the TUI).

pub use crate::screens::doctor::{Doctor, DoctorProps};

use crate::utils::doctor_diagnostic::{DiagnosticInfo, RipgrepMode};

/// Official `claude doctor` platform label (`process.platform}-{process.arch}`).
pub fn doctor_platform_label() -> String {
    let os = match std::env::consts::OS {
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// Shared with the doctor screen (`screens::doctor::doctor_search_status`).
pub fn doctor_search_status(diagnostic: &DiagnosticInfo) -> String {
    let status = if diagnostic.ripgrep_status.working {
        "OK"
    } else {
        "Not working"
    };
    let mode = match diagnostic.ripgrep_status.mode {
        RipgrepMode::Embedded => "bundled".to_string(),
        RipgrepMode::Builtin => "vendor".to_string(),
        RipgrepMode::System => diagnostic
            .ripgrep_status
            .system_path
            .clone()
            .unwrap_or_else(|| "system".to_string()),
    };
    format!("{status} ({mode})")
}

/// Render the text report printed by the official `claude doctor` CLI command.
pub fn format_doctor_cli_report(diagnostic: &DiagnosticInfo) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} doctor\n\n", crate::constants::product::NAME));
    out.push_str(&format!(
        "Running: {} ({})\n",
        diagnostic.installation_type.as_str(),
        diagnostic.version
    ));
    out.push_str(&format!("Platform: {}\n", doctor_platform_label()));
    out.push_str(&format!("Path: {}\n", diagnostic.installation_path));
    out.push_str(&format!(
        "Config install method: {}\n",
        diagnostic.config_install_method
    ));
    out.push_str(&format!("Search: {}\n", doctor_search_status(diagnostic)));
    out.push_str(&format!("Auto-updates: {}\n", diagnostic.auto_updates));

    if diagnostic.warnings.is_empty() {
        out.push_str("\nNo installation issues found.\n");
    } else {
        out.push_str("\nInstallation issues:\n");
        for warning in &diagnostic.warnings {
            out.push_str(&format!("- {}\n  Fix: {}\n", warning.issue, warning.fix));
        }
    }
    if let Some(recommendation) = &diagnostic.recommendation {
        out.push_str(&format!("\n{recommendation}\n"));
    }
    out.push_str(&format!(
        "\nFor a full setup checkup that can also fix issues, run /doctor in a {} session.\n",
        crate::constants::product::NAME
    ));
    out
}

/// Maps to CC `cli.tsx` `doctorHandler`: run the local diagnostic and exit
/// without entering the interactive TUI. Returns `None` for argv that is not
/// the `doctor` subcommand.
pub fn maybe_run_doctor_cli(argv: &[String]) -> Option<i32> {
    let doctor_index = argv.iter().position(|arg| arg == "doctor")?;
    let args = &argv[doctor_index + 1..];
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!(
            "Usage: cometix doctor [options]\n\nCheck the health of your CometixCode installation. Reads settings files in the\ncurrent directory without a trust prompt. For a full checkup that can also fix\nissues, run /doctor in a session.\n\nOptions:\n  -h, --help  Display help for command"
        );
        return Some(0);
    }
    if let Some(arg) = args.iter().find(|arg| arg.starts_with('-')) {
        eprintln!("error: unknown option '{arg}'");
        return Some(1);
    }
    let diagnostic = crate::utils::doctor_diagnostic::get_doctor_diagnostic();
    print!("{}", format_doctor_cli_report(&diagnostic));
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::doctor_diagnostic::{
        DiagnosticWarning, InstallationType, RipgrepStatus,
    };

    fn sample_diagnostic() -> DiagnosticInfo {
        DiagnosticInfo {
            installation_type: InstallationType::Development,
            version: "0.2.0".to_string(),
            installation_path: "/opt/cometix/target/debug/cometix".to_string(),
            invoked_binary: "cometix".to_string(),
            config_install_method: "not set".to_string(),
            auto_updates: "enabled".to_string(),
            has_update_permissions: None,
            multiple_installations: Vec::new(),
            warnings: Vec::new(),
            recommendation: None,
            package_manager: None,
            ripgrep_status: RipgrepStatus {
                working: true,
                mode: RipgrepMode::Embedded,
                system_path: None,
            },
        }
    }

    #[test]
    fn doctor_platform_label_matches_official_process_labels() {
        assert!(doctor_platform_label().contains('-'));
        assert!(doctor_platform_label().starts_with(
            match std::env::consts::OS {
                "windows" => "win32",
                other => other,
            }
        ));
    }

    #[test]
    fn doctor_search_status_maps_all_ripgrep_modes() {
        let mut diagnostic = sample_diagnostic();
        assert_eq!(doctor_search_status(&diagnostic), "OK (bundled)");
        diagnostic.ripgrep_status.mode = RipgrepMode::Builtin;
        assert_eq!(doctor_search_status(&diagnostic), "OK (vendor)");
        diagnostic.ripgrep_status.mode = RipgrepMode::System;
        diagnostic.ripgrep_status.system_path = Some("/usr/bin/rg".to_string());
        assert_eq!(doctor_search_status(&diagnostic), "OK (/usr/bin/rg)");
        diagnostic.ripgrep_status.working = false;
        assert_eq!(doctor_search_status(&diagnostic), "Not working (/usr/bin/rg)");
    }

    #[test]
    fn doctor_cli_report_reports_clean_and_warning_states() {
        let clean = format_doctor_cli_report(&sample_diagnostic());
        assert!(clean.starts_with("CometixCode doctor\n\n"));
        assert!(clean.contains("Running: development (0.2.0)"));
        assert!(clean.contains("Config install method: not set"));
        assert!(clean.contains("Search: OK (bundled)"));
        assert!(clean.contains("No installation issues found."));
        assert!(clean.contains("run /doctor in a CometixCode session."));

        let mut with_warning = sample_diagnostic();
        with_warning.warnings.push(DiagnosticWarning {
            issue: "Broken alias found".to_string(),
            fix: "Remove it".to_string(),
        });
        let report = format_doctor_cli_report(&with_warning);
        assert!(report.contains("Installation issues:"));
        assert!(report.contains("- Broken alias found"));
        assert!(report.contains("Fix: Remove it"));
        assert!(!report.contains("No installation issues found."));
    }

    #[test]
    fn doctor_cli_help_and_unknown_options_are_handled() {
        let help = maybe_run_doctor_cli(&[
            "cometix".to_string(),
            "doctor".to_string(),
            "--help".to_string(),
        ]);
        assert_eq!(help, Some(0));
        let unknown = maybe_run_doctor_cli(&[
            "cometix".to_string(),
            "doctor".to_string(),
            "--bogus".to_string(),
        ]);
        assert_eq!(unknown, Some(1));
        assert_eq!(maybe_run_doctor_cli(&["cometix".to_string()]), None);
    }
}
