//! Maps to: CC 2.1.332 `import` command.
//!
//! The target Claude Code build advertises the command surface but refuses to
//! run the importer:
//!
//! ```text
//! `claude import` is not yet available in this build. Run `claude` and use
//! /mcp or edit ~/.claude/settings.json directly.
//! ```
//!
//! This module reproduces that shipped contract instead of inventing an
//! importer that upstream 2.1.332 does not expose. Help output and option
//! errors are owned here so the command never falls into the generic
//! `Unimplemented:` envelope.

const HELP: &str = "Usage: cometix import [options] [source]

Import config from another AI coding agent into Claude Code

Arguments:
  source      Which agent to import from (codex, gemini, cursor)

Options:
  --dry-run   Show what would be imported without writing anything
  -h, --help  Display help for command
  --yes       Skip the interactive picker. In non-interactive mode, pass
              --yes=<digest> from the `/import` preview.";

const UNAVAILABLE: &str = "`cometix import` is not yet available in this build. Run `cometix` and use /mcp or edit ~/.claude/settings.json directly.";

/// Maps to: CC `import` commander entry. Returns `None` when argv is not this
/// subcommand.
pub fn maybe_run_import_cli(argv: &[String]) -> Option<i32> {
    let index = argv.iter().position(|arg| arg == "import")?;
    Some(run_import(&argv[index + 1..]))
}

fn run_import(args: &[String]) -> i32 {
    let mut options_done = false;
    let mut help = false;
    let mut unknown_option: Option<&str> = None;

    for (index, arg) in args.iter().enumerate() {
        if options_done {
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        if arg == "-h" || arg == "--help" {
            help = true;
            continue;
        }
        if arg == "--dry-run" || arg == "--yes" {
            continue;
        }
        if arg.starts_with("--yes=") {
            // Commander treats the optional-value `--yes=<digest>` form as
            // terminal: another option after it makes this token the unknown
            // option, while a positional after it is fine.
            let followed_by_option = args[index + 1..]
                .iter()
                .take_while(|next| next.as_str() != "--")
                .any(|next| next.starts_with('-'));
            if followed_by_option && unknown_option.is_none() {
                unknown_option = Some(arg);
            }
            continue;
        }
        if arg.starts_with('-') && unknown_option.is_none() {
            unknown_option = Some(arg);
        }
    }

    if help {
        println!("{HELP}");
        return 0;
    }
    if let Some(option) = unknown_option {
        // Commander's help scan still finds a later help token when parsing
        // already failed, even if `--` appears before it.
        if args
            .iter()
            .any(|candidate| candidate == "-h" || candidate == "--help")
        {
            println!("{HELP}");
            return 0;
        }
        eprintln!("error: unknown option '{option}'");
        return 1;
    }

    eprintln!("{UNAVAILABLE}");
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn help_and_unavailable_contracts_are_distinct() {
        assert_eq!(
            super::run_import(&["--help".to_string()]),
            0,
            "help must succeed"
        );
        assert_eq!(
            super::run_import(&["codex".to_string()]),
            1,
            "importer must stay unavailable"
        );
    }
}
