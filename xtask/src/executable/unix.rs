//! `chmod +x` for the hook scripts, in case the repo arrived by a
//! route that dropped the bit.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

/// Best-effort `chmod +x` for every regular file in `dir`, shelling out
/// rather than dragging in `nix` for one call. The README beside the
/// hooks is documentation, not a hook, and is skipped.
pub(crate) fn mark_all(dir: &Path) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
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
