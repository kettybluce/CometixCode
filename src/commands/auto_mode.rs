//! Maps to: CC `commands/auto-mode/*` and `cli/autoMode.ts`.
//!
//! The standalone `auto-mode` command owns its stdout/stderr and never enters
//! the interactive TUI. It reads only trusted settings sources, matching the
//! classifier's `userSettings` / `flagSettings` / `policySettings` merge.

use crate::utils::permissions::yolo_classifier::{
    AUTO_MODE_DEFAULTS_SENTINEL, AutoModeRules, build_default_external_system_prompt,
    get_default_external_auto_mode_rules, resolve_auto_mode_rules,
};
use crate::utils::settings::{
    SettingSource, get_auto_mode_config, get_settings_file_path_for_source,
    get_settings_for_source, update_settings_for_source,
};
use serde_json::{Map, Value};
use std::io::{self, BufRead, Write};

const ROOT_HELP: &str = "Usage: cometix auto-mode [options] [command]

Inspect or reset auto mode configuration

Options:
  -h, --help          Display help for command

Commands:
  config              Print the auto mode config in use as JSON: your settings
                      where set, defaults otherwise
  critique [options]  Get AI feedback on your custom auto mode rules
  defaults [options]  Print the default auto mode environment, allow, soft_deny,
                      and hard_deny rules as JSON
  help [command]      display help for command
  reset [options]     Reset auto mode to its defaults by removing the autoMode
                      section from your user settings file";

const DEFAULTS_HELP: &str = "Usage: cometix auto-mode defaults [options]

Print the default auto mode environment, allow, soft_deny, and hard_deny rules
as JSON

Options:
  -h, --help        Display help for command
  --label <prefix>  Show only rules whose label starts with this prefix
                    (case-insensitive)";

const CONFIG_HELP: &str = "Usage: cometix auto-mode config [options]

Print the auto mode config in use as JSON: your settings where set, defaults
otherwise

Options:
  -h, --help  Display help for command";

const RESET_HELP: &str = "Usage: cometix auto-mode reset [options]

Reset auto mode to its defaults by removing the autoMode section from your user
settings file

Options:
  -h, --help  Display help for command
  -y, --yes   Skip the confirmation prompt";

const CRITIQUE_HELP: &str = "Usage: cometix auto-mode critique [options]

Get AI feedback on your custom auto mode rules

Options:
  -h, --help       Display help for command
  --model <model>  Model to use";

const CRITIQUE_SYSTEM_PROMPT: &str =
    "You are an expert reviewer of auto mode classifier rules for Claude Code.

Claude Code has an \"auto mode\" that uses an AI classifier to decide whether tool calls should be auto-approved or require user confirmation. Users can write custom rules in four categories:

- **allow**: Actions the classifier should auto-approve
- **soft_deny**: Destructive/irreversible actions the classifier should block unless clear user intent authorizes them
- **hard_deny**: Security-boundary actions the classifier should block unconditionally (user intent does not clear these)
- **environment**: Context about the user's setup that helps the classifier make decisions

Your job is to critique the user's custom rules for clarity, completeness, and potential issues. The classifier is an LLM that reads these rules as part of its system prompt.

For each rule, evaluate:
1. **Clarity**: Is the rule unambiguous? Could the classifier misinterpret it?
2. **Completeness**: Are there gaps or edge cases the rule doesn't cover?
3. **Conflicts**: Do any of the rules conflict with each other?
4. **Actionability**: Is the rule specific enough for the classifier to act on?

Be concise and constructive. Only comment on rules that could be improved. If all rules look good, say so.";

/// Maps to: CC `auto-mode` commander entry. Returns `None` when argv is not
/// this subcommand.
pub fn maybe_run_auto_mode_cli(argv: &[String]) -> Option<i32> {
    let index = argv.iter().position(|arg| arg == "auto-mode")?;
    let args = &argv[index + 1..];
    Some(run_auto_mode(args))
}

fn run_auto_mode(args: &[String]) -> i32 {
    let Some(command) = args.first() else {
        eprintln!("{ROOT_HELP}");
        return 1;
    };
    if command == "-h" || command == "--help" {
        println!("{ROOT_HELP}");
        return 0;
    }
    if command == "help" {
        return print_help(args.get(1).map(String::as_str));
    }
    match command.as_str() {
        "defaults" => run_defaults(&args[1..]),
        "config" => run_config(&args[1..]),
        "reset" => run_reset(&args[1..]),
        "critique" => run_critique(&args[1..]),
        other => {
            eprintln!("error: unknown command '{other}'");
            1
        }
    }
}

fn print_help(command: Option<&str>) -> i32 {
    let text = match command {
        None | Some("auto-mode") => ROOT_HELP,
        Some("defaults") => DEFAULTS_HELP,
        Some("config") => CONFIG_HELP,
        Some("reset") => RESET_HELP,
        Some("critique") => CRITIQUE_HELP,
        Some(other) => {
            eprintln!("error: unknown command '{other}'");
            return 1;
        }
    };
    println!("{text}");
    0
}

fn run_defaults(args: &[String]) -> i32 {
    let mut label: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-h" || arg == "--help" {
            println!("{DEFAULTS_HELP}");
            return 0;
        }
        if let Some(value) = arg.strip_prefix("--label=") {
            label = Some(value.to_string());
        } else if arg == "--label" {
            let Some(value) = args.get(index + 1) else {
                eprintln!("error: option '--label <prefix>' argument missing");
                return 1;
            };
            index += 1;
            label = Some(value.clone());
        } else if arg.starts_with('-') {
            eprintln!("error: unknown option '{arg}'");
            return 1;
        }
        index += 1;
    }
    let rules = get_default_external_auto_mode_rules();
    let rules = label
        .as_deref()
        .map(|label| filter_rules(&rules, label))
        .unwrap_or(rules);
    print_rules(&rules)
}

fn run_config(args: &[String]) -> i32 {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{CONFIG_HELP}");
        return 0;
    }
    if let Some(arg) = args.iter().find(|arg| arg.starts_with('-')) {
        eprintln!("error: unknown option '{arg}'");
        return 1;
    }
    let defaults = get_default_external_auto_mode_rules();
    let config = get_auto_mode_config();
    let rules = AutoModeRules {
        allow: resolve_auto_mode_rules(
            config.as_ref().and_then(|config| config.allow.clone()),
            &defaults.allow,
        ),
        soft_deny: resolve_auto_mode_rules(
            config.as_ref().and_then(|config| config.soft_deny.clone()),
            &defaults.soft_deny,
        ),
        hard_deny: resolve_auto_mode_rules(
            config.as_ref().and_then(|config| config.hard_deny.clone()),
            &defaults.hard_deny,
        ),
        environment: resolve_auto_mode_rules(
            config
                .as_ref()
                .and_then(|config| config.environment.clone()),
            &defaults.environment,
        ),
    };
    print_rules(&rules)
}

fn print_rules(rules: &AutoModeRules) -> i32 {
    match serde_json::to_string_pretty(rules) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(error) => {
            eprintln!("Failed to serialize auto mode rules: {error}");
            1
        }
    }
}

/// Maps to: CC `b()` label filter. Labels are matched after removing `*` and
/// bracketed qualifiers, then taking the text before the first `:` or `[`.
fn filter_rules(rules: &AutoModeRules, filter: &str) -> AutoModeRules {
    let prefix = rule_label(filter);
    let prefix = if prefix.is_empty() {
        filter.to_lowercase()
    } else {
        prefix
    };
    let keep = |rule: &String| rule_label(rule).starts_with(&prefix);
    AutoModeRules {
        allow: rules
            .allow
            .iter()
            .filter(|rule| keep(rule))
            .cloned()
            .collect(),
        soft_deny: rules
            .soft_deny
            .iter()
            .filter(|rule| keep(rule))
            .cloned()
            .collect(),
        hard_deny: rules
            .hard_deny
            .iter()
            .filter(|rule| keep(rule))
            .cloned()
            .collect(),
        environment: rules
            .environment
            .iter()
            .filter(|rule| keep(rule))
            .cloned()
            .collect(),
    }
}

fn rule_label(rule: &str) -> String {
    let mut without_brackets = String::new();
    let mut in_brackets = false;
    for ch in rule.chars() {
        match ch {
            '[' => in_brackets = true,
            ']' if in_brackets => in_brackets = false,
            '*' if !in_brackets => {}
            _ if !in_brackets => without_brackets.push(ch),
            _ => {}
        }
    }
    let without_brackets = without_brackets.to_lowercase();
    let end = without_brackets
        .find(|ch| ch == ':' || ch == '[')
        .unwrap_or(without_brackets.len());
    without_brackets[..end].trim_end().to_string()
}

fn run_reset(args: &[String]) -> i32 {
    let mut yes = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-h" || arg == "--help" {
            println!("{RESET_HELP}");
            return 0;
        }
        if arg == "-y" || arg == "--yes" {
            yes = true;
        } else if arg.starts_with('-') {
            eprintln!("error: unknown option '{arg}'");
            return 1;
        }
        index += 1;
    }

    let Some(path) = get_settings_file_path_for_source(SettingSource::User) else {
        eprintln!("Could not resolve the user settings file path.");
        return 1;
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            eprintln!("Could not read {}: {error}", path.display());
            return 1;
        }
    };
    if raw.trim().is_empty() {
        println!(
            "Auto mode configuration is already at defaults — {} has no autoMode section.",
            path.display()
        );
        return 0;
    }
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => {
            eprintln!(
                "The settings file at {} contains invalid JSON — fix or remove it, then re-run reset.",
                path.display()
            );
            return 1;
        }
    };
    let Value::Object(object) = parsed else {
        eprintln!(
            "The settings file at {} contains invalid JSON — fix or remove it, then re-run reset.",
            path.display()
        );
        return 1;
    };
    let Some(auto_mode) = object.get("autoMode") else {
        println!(
            "Auto mode configuration is already at defaults — {} has no autoMode section.",
            path.display()
        );
        return 0;
    };

    println!(
        "This resets auto mode to its defaults by removing the autoMode section from {}:",
        path.display()
    );
    match auto_mode {
        Value::Object(sections) => {
            for (name, value) in sections {
                match value {
                    Value::Array(values) => {
                        println!("  - {name} ({} {})", values.len(), entry_word(values.len()))
                    }
                    _ => println!("  - {name}"),
                }
            }
        }
        _ => println!("  - autoMode (unrecognized value)"),
    }

    if !yes {
        print!("Reset auto mode configuration to defaults? [y/N] ");
        if io::stdout().flush().is_err() {
            eprintln!("Aborted.");
            return 1;
        }
        let mut answer = String::new();
        if io::stdin().lock().read_line(&mut answer).is_err() {
            eprintln!("Aborted.");
            return 1;
        }
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            eprintln!("Aborted.");
            return 1;
        }
    }

    let update = Map::from_iter([("autoMode".to_string(), Value::Null)]);
    if let Err(error) = update_settings_for_source(SettingSource::User, &update) {
        eprintln!("Failed to reset auto mode configuration: {error}");
        return 1;
    }
    println!(
        "Auto mode configuration reset to defaults — autoMode section removed from {}.",
        path.display()
    );
    println!("Run `cometix auto-mode config` to see the effective rules.");

    let still_applies = [SettingSource::Flag, SettingSource::Policy]
        .iter()
        .any(|source| {
            get_settings_for_source(*source)
                .and_then(|settings| settings.auto_mode)
                .is_some()
        });
    if still_applies {
        println!(
            "Auto mode rules from managed settings or --settings still apply. Reset only changes your user settings file."
        );
    }
    0
}

fn entry_word(count: usize) -> &'static str {
    if count == 1 { "entry" } else { "entries" }
}

fn run_critique(args: &[String]) -> i32 {
    let mut model: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-h" || arg == "--help" {
            println!("{CRITIQUE_HELP}");
            return 0;
        }
        if let Some(value) = arg.strip_prefix("--model=") {
            model = Some(value.to_string());
        } else if arg == "--model" {
            let Some(value) = args.get(index + 1) else {
                eprintln!("error: option '--model <model>' argument missing");
                return 1;
            };
            index += 1;
            model = Some(value.clone());
        } else if arg.starts_with('-') {
            eprintln!("error: unknown option '{arg}'");
            return 1;
        }
        index += 1;
    }

    let config = get_auto_mode_config();
    let has_custom = config.as_ref().is_some_and(|config| {
        [
            config.allow.as_deref(),
            config.soft_deny.as_deref(),
            config.hard_deny.as_deref(),
            config.environment.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|rules| rules.iter().any(|rule| rule != AUTO_MODE_DEFAULTS_SENTINEL))
    });
    if !has_custom {
        println!(
            "No custom auto mode rules found.\n\nAdd rules to your settings file under autoMode.{{allow, soft_deny, hard_deny, environment}}.\nRun `cometix auto-mode defaults` to see the default rules for reference."
        );
        return 0;
    }

    let defaults = get_default_external_auto_mode_rules();
    let mut custom_sections = String::new();
    for (name, custom, default) in [
        (
            "allow",
            config.as_ref().and_then(|config| config.allow.clone()),
            &defaults.allow,
        ),
        (
            "soft_deny",
            config.as_ref().and_then(|config| config.soft_deny.clone()),
            &defaults.soft_deny,
        ),
        (
            "hard_deny",
            config.as_ref().and_then(|config| config.hard_deny.clone()),
            &defaults.hard_deny,
        ),
        (
            "environment",
            config
                .as_ref()
                .and_then(|config| config.environment.clone()),
            &defaults.environment,
        ),
    ] {
        custom_sections.push_str(&critique_section(name, custom, default));
    }
    let prompt = build_default_external_system_prompt();
    let user_message = format!(
        "Here is the full classifier system prompt that the auto mode classifier receives:\n\n<classifier_system_prompt>\n{prompt}\n</classifier_system_prompt>\n\nHere are the user's custom rules (each section header notes whether they replace or extend the defaults):\n\n{custom_sections}\nPlease critique these custom rules."
    );
    let model = model.unwrap_or_else(crate::utils::model::model::get_main_loop_model);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Couldn't analyze rules: {error}");
            return 1;
        }
    };
    let result = runtime.block_on(async {
        crate::utils::side_query::side_query(crate::utils::side_query::SideQueryOptions {
            model,
            system: Some(crate::utils::side_query::SideQuerySystem::Text(
                CRITIQUE_SYSTEM_PROMPT.to_string(),
            )),
            messages: vec![crate::utils::side_query::user_text_message(user_message)],
            max_tokens: Some(4096),
            skip_system_prompt_prefix: true,
            is_non_interactive: true,
            query_source: "auto_mode_critique".to_string(),
            ..Default::default()
        })
        .await
    });
    match result {
        Ok(message) => {
            use anthropic_sdk::resources::messages::ContentBlock;
            let text = message
                .content
                .iter()
                .find_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .unwrap_or("No feedback came back. Try again.");
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("Couldn't analyze rules: {error}");
            1
        }
    }
}

fn critique_section(name: &str, custom: Option<Vec<String>>, defaults: &[String]) -> String {
    let Some(custom) = custom else {
        return String::new();
    };
    let rules = custom
        .iter()
        .filter(|rule| rule.as_str() != AUTO_MODE_DEFAULTS_SENTINEL)
        .cloned()
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return String::new();
    }
    let includes_defaults = custom
        .iter()
        .any(|rule| rule == AUTO_MODE_DEFAULTS_SENTINEL);
    let mut section = format!(
        "## {name} (custom rules {})\nCustom:\n{}\n\n",
        if includes_defaults {
            "added alongside the defaults"
        } else {
            "replacing defaults"
        },
        rules
            .iter()
            .map(|rule| format!("- {rule}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    section.push_str(if includes_defaults {
        "Defaults also in effect:\n"
    } else {
        "Defaults being replaced:\n"
    });
    section.push_str(
        &defaults
            .iter()
            .map(|rule| format!("- {rule}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    section.push_str("\n\n");
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_filter_matches_sanitized_prefix() {
        let rules = AutoModeRules {
            allow: vec!["Git Push Destination: push".into()],
            soft_deny: vec![
                "Git Destructive [named+specifics — **must name:** target]: force".into(),
                "Browser JS Exfil: x".into(),
            ],
            hard_deny: vec!["Data Exfiltration: y".into()],
            environment: vec!["**Network posture**: None".into()],
        };
        let filtered = filter_rules(&rules, "git");
        assert_eq!(filtered.allow.len(), 1);
        assert_eq!(filtered.soft_deny.len(), 1);
        assert!(filtered.hard_deny.is_empty());

        let stars = filter_rules(&rules, "**Network");
        assert_eq!(stars.environment.len(), 1);
        let brackets = filter_rules(&rules, "Git Destructive");
        assert_eq!(brackets.soft_deny.len(), 1);
    }

    #[test]
    fn critique_section_marks_replacing_and_alongside() {
        let defaults = vec!["default".to_string()];
        let replacing = critique_section("allow", Some(vec!["custom".to_string()]), &defaults);
        assert!(replacing.contains("custom rules replacing defaults"));
        assert!(!replacing.contains("Defaults also in effect"));
        let alongside = critique_section(
            "allow",
            Some(vec!["custom".to_string(), "$defaults".to_string()]),
            &defaults,
        );
        assert!(alongside.contains("custom rules added alongside the defaults"));
        assert!(alongside.contains("Defaults also in effect"));
        assert!(critique_section("allow", Some(vec!["$defaults".into()]), &defaults).is_empty());
    }
}
