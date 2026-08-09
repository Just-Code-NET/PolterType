//! `cargo xtask version` — bump or set the workspace version.
//!
//! Edits the three files that must stay in lock-step on a release
//! (`Cargo.toml`, `CHANGELOG.md`, `Cargo.lock`), so the release-cutter
//! need not remember the order. `docs/RELEASING.md` has the full flow.
//!
//! ```text
//! cargo xtask version                    # print current
//! cargo xtask version bump               # auto-bump
//! cargo xtask version set <NEW>          # exact set
//! cargo xtask version <subcommand> --dry-run
//! ```
//!
//! Auto-bump increments a trailing pre-release counter (`-beta.5` →
//! `-beta.6`) when there is one, and the patch component otherwise.
//! The rarer transitions — alpha → beta, dropping the suffix at 1.0 —
//! are worth doing explicitly with `set` rather than guessing.
//!
//! A missing or differently-shaped `## [Unreleased]` heading is a
//! warning, not an error, so `CHANGELOG.md` need not exist.
//!
//! It deliberately does **not** commit, tag or push — a release commit
//! is the moment to catch a wrong bump, not after the tag has hit CI —
//! does not talk to the network, and pulls in no `semver`/`regex`
//! dependency for a version shape the hand-rolled parser below covers
//! in ~30 lines.

use anyhow::{Result, bail};

mod apply;
mod enums;
mod files;
mod semver;
mod types;

pub(crate) use apply::*;
pub(crate) use enums::*;
pub(crate) use files::*;
pub(crate) use semver::*;
pub(crate) use types::*;

/// Entry point — dispatches `version [bump|set <V>] [--dry-run]`.
pub fn run(rest: &[String]) -> Result<()> {
    // Strip an optional trailing `--dry-run` flag. We accept it
    // anywhere after the subcommand so users don't have to remember
    // exact ordering.
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

#[cfg(test)]
mod tests;
