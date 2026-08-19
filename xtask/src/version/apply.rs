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
            // The parser accepts everything we would ever want to set,
            // so a string it rejects is almost certainly a typo.
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

    // Anchored on the leading `version       = ` so a version string
    // inside a doc comment is not replaced.
    let new_body = replace_version_line(&body, &current, &next)?;
    fs::write(&cargo_toml, new_body).with_context(|| format!("write {}", cargo_toml.display()))?;
    println!("  ✓ Cargo.toml");

    // A missing or mismatched CHANGELOG heading warns rather than
    // failing, so a fresh checkout without one still works.
    let changelog = workspace_root()?.join("CHANGELOG.md");
    match update_changelog(&changelog, &current, &next) {
        Ok(true) => println!("  ✓ CHANGELOG.md"),
        Ok(false) => println!(
            "  · CHANGELOG.md heading not found — skipping (add `## [Unreleased] — {current}` to enable)"
        ),
        Err(e) => println!("  · CHANGELOG.md update failed: {e} — skipping"),
    }

    // `cargo check` refreshes Cargo.lock. Rewriting the lock by hand
    // would drift, and `cargo metadata` means cargo as a dependency.
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
