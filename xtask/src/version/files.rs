//! Cargo.toml / CHANGELOG edits and workspace paths.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn read_version(cargo_toml_body: &str) -> Result<String> {
    // We deliberately don't pull in toml-edit just for one read —
    // the workspace.package.version line has a fixed shape that's
    // been the same since Phase 0 of the project.
    for line in cargo_toml_body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // Match e.g. `version       = "0.1.0-beta.6"` or
            // `version="..."`. Anchored on the start of trimmed
            // text so a stray `version` substring inside a comment
            // doesn't match.
            let after_eq = rest.trim_start().strip_prefix('=').map(str::trim_start);
            if let Some(after_eq) = after_eq {
                if let Some(quoted) = after_eq.strip_prefix('"') {
                    if let Some(end) = quoted.find('"') {
                        return Ok(quoted[..end].to_owned());
                    }
                }
            }
        }
    }
    bail!("could not find a `version = \"...\"` line in Cargo.toml")
}

pub(crate) fn replace_version_line(body: &str, current: &str, next: &str) -> Result<String> {
    // Replace the FIRST occurrence of `version = "<current>"`.
    // Doing a global string replace would also rewrite e.g. a
    // `poltertype-types = { version = "0.1.0-beta.6" }` inside a dep entry
    // — we only want the workspace.package one, which is the first
    // version line in our Cargo.toml shape.
    let needle = format!("version       = \"{current}\"");
    if let Some(found) = body.find(&needle) {
        let mut out = String::with_capacity(body.len());
        out.push_str(&body[..found]);
        out.push_str(&format!("version       = \"{next}\""));
        out.push_str(&body[found + needle.len()..]);
        return Ok(out);
    }
    // Fallback for Cargo.toml shapes that don't pad the equals
    // (e.g. someone reformatted the file). Less surgical, but only
    // matches the FIRST `version = "<current>"` so we still don't
    // touch dep version pins later in the file.
    let alt_needle = format!("version = \"{current}\"");
    if let Some(found) = body.find(&alt_needle) {
        let mut out = String::with_capacity(body.len());
        out.push_str(&body[..found]);
        out.push_str(&format!("version = \"{next}\""));
        out.push_str(&body[found + alt_needle.len()..]);
        return Ok(out);
    }
    bail!("could not find `version = \"{current}\"` line to replace in Cargo.toml")
}

pub(crate) fn update_changelog(path: &Path, current: &str, next: &str) -> Result<bool> {
    let Ok(body) = fs::read_to_string(path) else {
        return Ok(false);
    };
    // We only auto-update `## [Unreleased] — <ver>` headings — that's
    // the convention this repo follows. A user with a different
    // CHANGELOG shape gets the warning message and can edit by hand.
    let needle = format!("## [Unreleased] — {current}");
    let Some(found) = body.find(&needle) else {
        return Ok(false);
    };
    let mut out = String::with_capacity(body.len());
    out.push_str(&body[..found]);
    out.push_str(&format!("## [Unreleased] — {next}"));
    out.push_str(&body[found + needle.len()..]);
    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

pub(crate) fn workspace_root() -> Result<PathBuf> {
    // The script is always invoked via `cargo xtask`, which sets
    // `CARGO_MANIFEST_DIR` to the xtask crate's directory. The
    // workspace root is one level up.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .context("CARGO_MANIFEST_DIR not set — run via `cargo xtask`, not the binary directly")?;
    Ok(PathBuf::from(manifest)
        .parent()
        .context("CARGO_MANIFEST_DIR has no parent")?
        .to_owned())
}

pub(crate) fn workspace_cargo_toml() -> Result<PathBuf> {
    Ok(workspace_root()?.join("Cargo.toml"))
}

pub(crate) fn cargo_bin() -> String {
    // Honour `CARGO` if cargo set it (it always does when run via
    // `cargo xtask`); fall back to the bare command for the rare
    // case of running the xtask binary directly.
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}
