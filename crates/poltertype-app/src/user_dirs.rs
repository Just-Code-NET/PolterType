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

/// Resolve the user wordlists directory, creating it if missing.
///
/// The `README.txt` is seeded only on actual creation, so a user may
/// delete, rename or replace it without it coming back.
pub(crate) fn ensure_user_wordlist_dir() -> anyhow::Result<PathBuf> {
    let dir = poltertype_core::layouts::user_wordlist_dir()
        .context("could not determine user-config directory")?;
    let needs_seed = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create wordlists dir at {}", dir.display()))?;
    if needs_seed {
        let readme = dir.join("README.txt");
        // Best-effort: a missing README must not block opening the
        // folder.
        if let Err(e) = std::fs::write(&readme, USER_WORDLISTS_README) {
            warn!(?e, ?readme, "could not seed README in wordlists folder");
        }
    }
    Ok(dir)
}

/// Resolve the user layouts directory, creating it if missing. The
/// `README.txt` carrying the TOML schema is seeded once, like the
/// wordlists one.
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
