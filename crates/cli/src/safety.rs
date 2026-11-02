//! `M63-21` — the safe path is the short path: a change is previewed unless you say `--yes`.
//!
//! Every mutating verb (`dh module on|off`, `dh supply recompute|route|complete`) now prints the
//! exact request it would send and exits `0` unless `--yes` is given. Three reasons, in the order they
//! matter:
//!
//! 1. **A wrong write is not a wrong read.** `dh supply complete` pays a patronage once; `dh module
//!    off` withdraws a surface from readers. A mistyped id should cost a re-run, not a revert.
//! 2. **The preview is the documentation.** `would PATCH http://…/api/v1/modules/concierge
//!    {"enabled":false}` tells the operator which door, which body, and — when they paste it into an
//!    issue — exactly what was attempted.
//! 3. **Scripts are explicit by construction.** A cron entry that forgot `--yes` fails visibly at its
//!    first run with a preview and no write, rather than silently halving the catalogue.
//!
//! `--dry-run` exists as well, and is the default: it is what a generated script should say, so the
//! intent is visible in the script rather than implied by a missing flag. Passing both is refused by
//! the argument parser (`conflicts_with`), because "apply" and "do not apply" is not a question with
//! a tie-break.

use crate::rec::Failure;
use crate::Cli;

/// Decide whether a mutating verb may proceed.
///
/// `Ok(true)` — apply. `Ok(false)` — the caller must return success without sending anything.
pub fn mutating(cli: &Cli, preview: &str) -> Result<bool, Failure> {
    if cli.dry_run {
        println!("would {preview}");
        return Ok(false);
    }
    if cli.yes {
        return Ok(true);
    }
    println!("would {preview}");
    println!("re-run with --yes to apply (nothing was changed)");
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn cli(args: &[&str]) -> Cli {
        let mut full = vec!["dh"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full).expect("a parseable command line")
    }

    #[test]
    fn without_yes_a_change_is_not_applied() {
        let parsed = cli(&["module", "off", "arena"]);
        assert!(!mutating(&parsed, "PATCH /x").expect("decide"));
    }

    #[test]
    fn with_yes_a_change_is_applied() {
        let parsed = cli(&["module", "off", "arena", "--yes"]);
        assert!(mutating(&parsed, "PATCH /x").expect("decide"));
    }

    #[test]
    fn dry_run_never_applies_even_beside_yes() {
        let parsed = cli(&["module", "off", "arena", "--dry-run"]);
        assert!(!mutating(&parsed, "PATCH /x").expect("decide"));
        // The pair is refused by the parser (`conflicts_with = "yes"`) rather than silently
        // resolved — there is no tie-break between "apply" and "do not apply".
        let both = Cli::try_parse_from(["dh", "module", "off", "arena", "--yes", "--dry-run"]);
        assert!(
            both.is_err(),
            "apply and do-not-apply is not a question with a tie-break"
        );
    }
}
