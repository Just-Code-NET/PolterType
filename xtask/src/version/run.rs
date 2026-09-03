//! Argument parsing for `cargo xtask version`.

use anyhow::{Result, bail};

use super::*;

/// Entry point — dispatches `version [bump|set <V>] [--dry-run]`.
pub(crate) fn run(rest: &[String]) -> Result<()> {
    // `--dry-run` is accepted anywhere after the subcommand.
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    let positional: Vec<&str> = rest
        .iter()
        .filter(|a| *a != "--dry-run")
        .map(String::as_str)
        .collect();

    match positional.as_slice() {
        [] => print_current(),
        ["bump"] => apply_change(Change::Bump, dry_run),
        ["set", new] => apply_change(Change::Set((*new).to_owned()), dry_run),
        ["bump", _, ..] | ["set", _, _, ..] => {
            bail!("too many arguments — see `cargo xtask help`");
        }
        ["set"] => bail!("`cargo xtask version set` needs an argument, e.g. `set 0.1.0-beta.7`"),
        [other, ..] => bail!("unknown version subcommand `{other}` (expected `bump` or `set`)"),
    }
}
