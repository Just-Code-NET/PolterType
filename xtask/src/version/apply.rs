//! Executing a version change across the workspace.

use super::*;
use anyhow::{Context, Result, bail};
use std::fs;
use std::process::Command;

pub(crate) fn print_current() -> Result<()> {
    let path = workspace_cargo_toml()?;
    let body = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cur =
        read_version(&body).with_context(|| format!("locate version in {}", path.display()))?;
    println!("{cur}");
    Ok(())
}

pub(crate) fn apply_change(change: Change, dry_run: bool) -> Result<()> {
    let cargo_toml = workspace_cargo_toml()?;
    let body = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("read {}", cargo_toml.display()))?;
    let current = read_version(&body)
        .with_context(|| format!("locate version in {}", cargo_toml.display()))?;

    let next = match &change {
        Change::Bump => bump(&current)?,
        Change::Set(new) => {
            // Light validation so a typo doesn't leak into the file.
            // The parser accepts everything we'd ever want to set;
            // a string it rejects is almost certainly a mistake.
            parse(new).with_context(|| format!("`{new}` is not a recognised version shape"))?;
            new.clone()
        }
    };

    if next == current {
        println!("version already {current} — nothing to do");
        return Ok(());
    }

    println!(
        "{current} → {next}{}",
        if dry_run { "  (dry-run)" } else { "" }
    );

    if dry_run {
        return Ok(());
    }

    // Step 1: rewrite Cargo.toml — single line, surgical replace
    // anchored on the leading `version       = ` so we don't
    // accidentally replace a version string inside a doc comment.
    let new_body = replace_version_line(&body, &current, &next)?;
    fs::write(&cargo_toml, new_body).with_context(|| format!("write {}", cargo_toml.display()))?;
    println!("  ✓ Cargo.toml");

    // Step 2: update CHANGELOG.md heading if present. Missing /
    // mismatched is a warning, not an error — keeps the script
    // useful in a fresh checkout that hasn't grown a CHANGELOG
    // yet, and avoids forcing every consumer of the script to
    // adopt our exact heading shape.
    let changelog = workspace_root()?.join("CHANGELOG.md");
    match update_changelog(&changelog, &current, &next) {
        Ok(true) => println!("  ✓ CHANGELOG.md"),
        Ok(false) => println!(
            "  · CHANGELOG.md heading not found — skipping (add `## [Unreleased] — {current}` to enable)"
        ),
        Err(e) => println!("  · CHANGELOG.md update failed: {e} — skipping"),
    }

    // Step 3: refresh Cargo.lock by running `cargo check`. Any
    // alternative (manually rewriting the lock, calling cargo
    // metadata) would either drift or pull in cargo as a dep —
    // shelling out is honest about what we're doing.
    println!("  · refreshing Cargo.lock via `cargo check --workspace` ...");
    let status = Command::new(cargo_bin())
        .args(["check", "--workspace", "--quiet"])
        .current_dir(workspace_root()?)
        .status()
        .context("spawn `cargo check`")?;
    if !status.success() {
        bail!(
            "`cargo check --workspace` failed — Cargo.lock may be out of sync with the new version"
        );
    }
    println!("  ✓ Cargo.lock");

    println!();
    println!("Next steps (see docs/RELEASING.md for the full checklist):");
    println!("  git diff Cargo.toml Cargo.lock CHANGELOG.md");
    println!("  git add Cargo.toml Cargo.lock CHANGELOG.md");
    println!("  git commit -m \"release: v{next}\"");
    println!("  git tag v{next}");
    println!("  git push origin HEAD --tags");

    Ok(())
}
