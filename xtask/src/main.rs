//! `cargo xtask` — internal helper commands. Each subcommand's own
//! module carries the detail; this is the index.
//!
//! * `wordlists fetch` — re-download and re-process the embedded
//!   dictionaries. Sources in `data/wordlists/CREDITS.md`.
//! * `hooks install` / `uninstall` — wire the versioned git hooks under
//!   `.githooks/`.
//! * `assets icon-png <path> [--size N]` — render the app icon for the
//!   release installers.
//! * `manifest [keygen | sign | verify | payload]` — sign `latest.json`
//!   so the updater can prove the manifest came from us and not merely
//!   from whoever can publish a GitHub release.
//! * `version [bump | set <X.Y.Z>] [--dry-run]` — bump the workspace
//!   version in lock-step across `Cargo.toml`, `CHANGELOG.md` and
//!   `Cargo.lock`.
//!
//! `docs/RELEASING.md` has the flow the last two fit into.

#![allow(clippy::unwrap_used, clippy::expect_used)] // build/dev tool

mod assets;
mod hunspell;
mod manifest;
mod version;

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

mod consts;
mod enums;
mod hooks;
mod paths;
mod types;
mod wordlists;

use consts::*;
use enums::*;
use hooks::*;
use paths::*;
use types::*;
use wordlists::*;

fn main() -> Result<()> {
    let rest: Vec<String> = std::env::args().skip(1).collect();
    match (
        rest.first().map(String::as_str),
        rest.get(1).map(String::as_str),
    ) {
        (Some("help") | None, _) => {
            print_help();
            Ok(())
        }
        (Some("wordlists"), Some("fetch")) => fetch_wordlists(),
        (Some("hooks"), Some("install")) => install_hooks(),
        (Some("hooks"), Some("uninstall")) => uninstall_hooks(),
        (Some("assets"), Some("icon-png")) => render_icon_command(&rest[2..]),
        (Some("manifest"), _) => manifest::run(&rest[1..]),
        (Some("version"), _) => version::run(&rest[1..]),
        (Some(other), _) => bail!("unknown xtask command: {other} (try `cargo xtask help`)"),
    }
}

fn print_help() {
    println!("xtask commands:");
    println!("  help                  Show this list.");
    println!("  wordlists fetch       Re-download and re-process the embedded dictionaries.");
    println!("  hooks install         Wire `.githooks/` into this clone (sets core.hooksPath).");
    println!("  hooks uninstall       Unset core.hooksPath (revert to default `.git/hooks/`).");
    println!("  assets icon-png <out> [--size N]");
    println!("                         Render the app icon as a PNG (default size 1024).");
    println!("  manifest              Sign / verify the release manifest (see `manifest` alone");
    println!("                         for the subcommands). Signing happens on the");
    println!("                         maintainer's machine, never in CI.");
    println!("  version               Print the current workspace version.");
    println!("  version bump          Bump the workspace version (auto: pre-release counter,");
    println!("                         else patch). Updates Cargo.toml, CHANGELOG.md, Cargo.lock.");
    println!("  version set <X.Y.Z>   Set the workspace version exactly. Same files updated.");
    println!("  version <subcmd> --dry-run   Print what would change without writing.");
}

/// Parse `<out-path> [--size N]` and render the icon.
///
/// Tiny ad-hoc parser instead of a clap dep — we only have one flag,
/// and the xtask crate has been resolutely zero-config so far.
fn render_icon_command(args: &[String]) -> Result<()> {
    let mut out_path: Option<PathBuf> = None;
    let mut size: u32 = 1024;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let v = args
                    .get(i + 1)
                    .context("--size needs a value (e.g. --size 256)")?;
                size = v
                    .parse()
                    .with_context(|| format!("--size {v}: not a u32"))?;
                i += 2;
            }
            other if !other.starts_with('-') && out_path.is_none() => {
                out_path = Some(PathBuf::from(other));
                i += 1;
            }
            other => bail!(
                "unexpected argument {other:?} (usage: cargo xtask assets icon-png <out-path> [--size N])"
            ),
        }
    }
    let out = out_path.context("missing output path (cargo xtask assets icon-png <out>)")?;
    assets::render_app_icon(size, &out)?;
    println!("rendered {}×{} icon to {}", size, size, out.display());
    Ok(())
}
