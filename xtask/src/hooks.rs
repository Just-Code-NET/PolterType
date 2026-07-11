//! `.githooks/` install / uninstall.

use crate::*;
use anyhow::{Context, Result, bail};
use std::fs::{self};
use std::path::Path;
use std::process::Command;

/// Wire the versioned `.githooks/` directory into the local clone by
/// setting `core.hooksPath`. This is the entire install — Git itself
/// runs every executable in that directory whose name matches a hook
/// stage, so we don't need to touch `.git/hooks/` at all (and stay
/// out of its way for users who already keep something there).
///
/// We also re-`chmod +x` the scripts on POSIX after the config write,
/// in case someone fetched the repo via a tool that didn't preserve
/// the executable bit (rare but happens with raw zip downloads).
pub(crate) fn install_hooks() -> Result<()> {
    let root = repo_root()?;
    let hooks_dir = root.join(".githooks");
    if !hooks_dir.exists() {
        bail!(
            "expected hooks directory at {} — refusing to set core.hooksPath to a missing path",
            hooks_dir.display()
        );
    }

    // Path stored in `git config` is interpreted relative to the
    // working tree root, so `.githooks` (no leading slash) is correct
    // and portable across platforms.
    let status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(&root)
        .status()
        .context("invoke `git config core.hooksPath`")?;
    if !status.success() {
        bail!("`git config core.hooksPath .githooks` failed (status: {status})");
    }

    #[cfg(unix)]
    chmod_executable(&hooks_dir)?;

    println!("poltertype hooks installed:");
    println!("  pre-commit  →  cargo fmt --all -- --check");
    println!("  pre-push    →  cargo build --workspace --all-targets");
    println!();
    println!("Bypass any single run with `git commit --no-verify` / `git push --no-verify`.");
    Ok(())
}

/// Inverse of `install_hooks`: drop the `core.hooksPath` config so
/// Git falls back to its default (`.git/hooks/`, empty in fresh
/// clones). `--unset` is a no-op if the config wasn't set, but git
/// returns exit-code 5 in that case — we suppress it to keep the
/// command idempotent ("uninstall what isn't installed → success").
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

/// Best-effort `chmod +x` for every regular file inside `dir`.
/// Avoids dragging the `nix` crate in for a one-call use; we just
/// shell out to `chmod` which is on every Unix that runs Git anyway.
/// On non-Unix this whole function is `cfg`-skipped — Git for Windows
/// runs hooks via its bundled `sh.exe` regardless of file mode.
#[cfg(unix)]
pub(crate) fn chmod_executable(dir: &Path) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Skip the README — it's documentation, not a hook.
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.starts_with("README"))
        {
            continue;
        }
        let _ = Command::new("chmod").arg("+x").arg(&path).status();
    }
    Ok(())
}
