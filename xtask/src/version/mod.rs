//! `cargo xtask version` — bump or set the workspace version.
//!
//! Edits the three files that need to stay in lock-step on every
//! release (`Cargo.toml`, `CHANGELOG.md`, `Cargo.lock`) so the
//! release-cutter doesn't have to remember the order or the exact
//! patterns. See `docs/RELEASING.md` for the full release flow this
//! command fits into.
//!
//! ## Surface
//!
//! ```text
//! cargo xtask version                    # print current
//! cargo xtask version bump               # auto-bump
//! cargo xtask version set <NEW>          # exact set
//! cargo xtask version <subcommand> --dry-run
//! ```
//!
//! "Auto-bump" rule: if the current version has a pre-release suffix
//! shaped like `-<word>.<N>` (e.g. `-beta.5`), increment the
//! trailing counter (`-beta.6`). Otherwise, increment the patch
//! component (`1.2.3` → `1.2.4`). This matches the project's actual
//! lifecycle — every poltertype release so far has been a bump of
//! the `alpha.<N>` / `beta.<N>` counter, with the major/minor/patch
//! transitions ("alpha → beta", "beta → rc", "drop suffix on 1.0")
//! happening rarely enough that they're worth doing explicitly with
//! `set` rather than guessing in the script.
//!
//! ## What the bump touches
//!
//! 1. `Cargo.toml` — `[workspace.package].version = "..."`
//! 2. `CHANGELOG.md` — top-level `## [Unreleased] — <ver>` heading
//!    if present (skipped with a warning if it isn't, so the file
//!    isn't required to exist or follow a particular shape).
//! 3. `Cargo.lock` — refreshed via `cargo check --workspace`.
//!
//! ## What it deliberately does NOT do
//!
//! * Commit, tag, or push. Release commits should be reviewed
//!   manually — that's the moment to catch a wrong bump or a
//!   missing changelog entry, not after the tag has hit CI. The
//!   doc at `docs/RELEASING.md` walks through the commit + tag
//!   step explicitly.
//! * Talk to the network. Pre-release validation (does this version
//!   already exist as a tag?) is the user's job — `git tag -l` is
//!   one line in the release checklist.
//! * Pull in `semver` / `regex` deps. The version shapes we ship
//!   are a small subset; the hand-rolled parser below covers them
//!   in ~30 lines.

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
