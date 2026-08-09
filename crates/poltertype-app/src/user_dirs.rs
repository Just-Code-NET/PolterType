//! User-facing data directories and "open in file manager".

use std::path::PathBuf;

use anyhow::Context;
use tracing::{debug, warn};

use crate::consts::*;

pub(crate) fn open_path(path: &std::path::Path, what: &str) {
    debug!(?path, "opening {what}");
    if let Err(e) = opener::open(path) {
        warn!(?e, ?path, "could not open {what} in default app");
    }
}

/// Resolve the user wordlists directory, creating it if missing and
/// seeding a `README.txt` on first creation so the folder explains
/// which files are recognised.
///
/// Seeded only on actual creation, so a user may delete, rename or
/// replace the README without it coming back.
pub(crate) fn ensure_user_wordlist_dir() -> anyhow::Result<PathBuf> {
    let dir = poltertype_core::layouts::user_wordlist_dir()
        .context("could not determine user-config directory")?;
    let needs_seed = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create wordlists dir at {}", dir.display()))?;
    if needs_seed {
        let readme = dir.join("README.txt");
        // Best-effort write — failure is logged but doesn't block
        // opening the folder. The directory itself is the value.
        if let Err(e) = std::fs::write(&readme, USER_WORDLISTS_README) {
            warn!(?e, ?readme, "could not seed README in wordlists folder");
        }
    }
    Ok(dir)
}

/// Resolve the user layouts directory, creating it if missing and
/// seeding a `README.txt` with the TOML schema on first creation.
/// Same single-shot behaviour as the wordlists README.
pub(crate) fn ensure_user_layout_dir() -> anyhow::Result<PathBuf> {
    let dir = poltertype_core::layouts::user_layout_dir()
        .context("could not determine user-config directory")?;
    let needs_seed = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create layouts dir at {}", dir.display()))?;
    if needs_seed {
        let readme = dir.join("README.txt");
        if let Err(e) = std::fs::write(&readme, USER_LAYOUTS_README) {
            warn!(?e, ?readme, "could not seed README in layouts folder");
        }
    }
    Ok(dir)
}
