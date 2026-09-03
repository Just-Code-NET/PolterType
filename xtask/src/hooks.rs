//! `.githooks/` install / uninstall.

use crate::*;
use anyhow::{Context, Result, bail};
use std::process::Command;

/// Wire the versioned `.githooks/` directory into the local clone by
/// setting `core.hooksPath`, leaving `.git/hooks/` untouched for
/// anyone who keeps something there.
///
/// The scripts are re-`chmod +x`ed on POSIX afterwards, in case the
/// repo arrived by a route that dropped the executable bit.
pub(crate) fn install_hooks() -> Result<()> {
    let root = repo_root()?;
    let hooks_dir = root.join(".githooks");
    if !hooks_dir.exists() {
        bail!(
            "expected hooks directory at {} — refusing to set core.hooksPath to a missing path",
            hooks_dir.display()
        );
    }

    // `git config` interprets this relative to the working tree root,
    // so `.githooks` with no leading slash is the portable spelling.
    let status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(&root)
        .status()
        .context("invoke `git config core.hooksPath`")?;
    if !status.success() {
        bail!("`git config core.hooksPath .githooks` failed (status: {status})");
    }

    executable::mark_all(&hooks_dir)?;

    println!("poltertype hooks installed:");
    println!("  pre-commit  →  cargo fmt --all -- --check");
    println!("  pre-push    →  cargo build --workspace --all-targets");
    println!();
    println!("Bypass any single run with `git commit --no-verify` / `git push --no-verify`.");
    Ok(())
}

/// Inverse of `install_hooks`: drop `core.hooksPath` so Git falls back
/// to `.git/hooks/`. Git exits 5 when the key was not set, which is
/// suppressed to keep "uninstall what isn't installed" a success.
pub(crate) fn uninstall_hooks() -> Result<()> {
    let root = repo_root()?;
    let output = Command::new("git")
        .args(["config", "--unset", "core.hooksPath"])
        .current_dir(&root)
        .output()
        .context("invoke `git config --unset core.hooksPath`")?;
    // git returns 5 if the key wasn't set — treat as success.
    let code = output.status.code().unwrap_or_default();
    if !output.status.success() && code != 5 {
        bail!(
            "`git config --unset core.hooksPath` failed (status: {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("poltertype hooks uninstalled — Git will use the default `.git/hooks/` from now on.");
    Ok(())
}
