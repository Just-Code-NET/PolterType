//! One-shot adoption of the legacy `kb-switcher` config directory.
//!
//! The 0.1.x releases shipped under the old name and kept user data
//! in the ProjectDirs tree for `dev.opensource.kb-switcher`
//! (`~/.config/kb-switcher` on Linux). The rebrand moved the app id
//! to `dev.opensource.poltertype`, which would silently reset every
//! existing user's settings and orphan their custom wordlist /
//! layout overlays. On first launch — no `config.toml` in the new
//! location yet — we copy the legacy tree across. The legacy
//! directory itself is left untouched as a backup.

use directories::ProjectDirs;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// Former app id, kept only so upgrades can find the old tree.
const LEGACY_APP_NAME: &str = "kb-switcher";

/// Migrate the legacy config tree into `new_config_dir` (the parent
/// of `new_config_path`). Returns `true` when a legacy `config.toml`
/// now exists at `new_config_path`, i.e. the caller should re-read it.
pub(crate) fn migrate_legacy_config(new_config_path: &Path) -> bool {
    let Some(new_dir) = new_config_path.parent() else {
        return false;
    };
    let Some(legacy) = ProjectDirs::from("dev", "opensource", LEGACY_APP_NAME) else {
        return false;
    };
    migrate_dir(legacy.config_dir(), new_dir) && new_config_path.is_file()
}

/// Testable core: copy `legacy_dir` into `new_dir` when — and only
/// when — the legacy tree has a `config.toml` and the new one does
/// not. Files already present in `new_dir` are never overwritten.
pub(crate) fn migrate_dir(legacy_dir: &Path, new_dir: &Path) -> bool {
    if !legacy_dir.join("config.toml").is_file() || new_dir.join("config.toml").is_file() {
        return false;
    }
    match copy_tree(legacy_dir, new_dir) {
        Ok(copied) => {
            info!(
                from = %legacy_dir.display(),
                to = %new_dir.display(),
                copied,
                "migrated legacy kb-switcher config directory (original left in place)"
            );
            true
        }
        Err(e) => {
            warn!(
                from = %legacy_dir.display(),
                to = %new_dir.display(),
                err = %e,
                "legacy config migration failed; starting with defaults"
            );
            false
        }
    }
}

/// Recursively copy `src` into `dst`, skipping destination files that
/// already exist. Returns how many files were copied.
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<usize> {
    fs::create_dir_all(dst)?;
    let mut copied = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copied += copy_tree(&from, &to)?;
        } else if !to.exists() {
            fs::copy(&from, &to)?;
            copied += 1;
        }
    }
    Ok(copied)
}
