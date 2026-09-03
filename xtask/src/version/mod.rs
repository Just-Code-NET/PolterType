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
//! It deliberately does **not** commit, tag or push: a release commit
//! is the moment to catch a wrong bump, not after the tag has hit CI.

mod apply;
mod enums;
mod files;
mod run;
mod semver;
mod types;

pub(crate) use apply::*;
pub(crate) use enums::*;
pub(crate) use files::*;
pub(crate) use semver::*;
pub(crate) use types::*;

pub(crate) use run::run;

#[cfg(test)]
mod tests;
