//! Migrations that run against an existing user config: adoption of
//! the legacy `kb-switcher` config directory, and retiring the
//! default app skip-list that v0.4.1 and earlier wrote into every
//! `config.toml`.
//!
//! The 0.1.x releases shipped under the old name and kept user data
//! in the ProjectDirs tree for `dev.opensource.kb-switcher`
//! (`~/.config/kb-switcher` on Linux). The rebrand moved the app id
//! to `dev.opensource.poltertype`, which would silently reset every
//! existing user's settings and orphan their custom wordlist /
//! layout overlays. On first launch — no `config.toml` in the new
//! location yet — we copy the legacy tree across. The legacy
//! directory itself is left untouched as a backup.

use super::consts::LEGACY_DEFAULT_DISABLED_APPS;
use super::types::Settings;
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

/// Clear `[exceptions].disabled_apps` when it is still, verbatim, the
/// list PolterType used to ship as a default. Returns `true` when the
/// settings were changed and the caller should persist them.
///
/// Why an existing config needs touching at all: the old default was
/// not a suggestion the user could ignore — it was *written into their
/// file* on first launch, all 69 entries of it. Shipping an empty
/// default in the binary therefore fixes nothing for anyone who
/// already ran an older build; their `config.toml` still spells out
/// the list, and PolterType would go on being mute in their editor
/// forever. The fix has to reach into the file.
///
/// Why only on an exact match: a skip-list somebody curated is a
/// deliberate statement about where they don't want us typing, and
/// clobbering it would be a far worse bug than the one we're fixing.
/// Set equality (order- and duplicate-insensitive — TOML round-trips
/// don't promise order) against the frozen historical list is the
/// narrowest test that catches every untouched config and no touched
/// one. Remove a single entry by hand and we leave you alone.
///
/// Idempotent by construction: once cleared, the list is empty and can
/// never match a 69-element set again, so there is no migration flag to
/// keep and no schema bump to coordinate.
pub(crate) fn retire_default_skip_list(settings: &mut Settings) -> bool {
    let current: std::collections::BTreeSet<&str> = settings
        .exceptions
        .disabled_apps
        .iter()
        .map(String::as_str)
        .collect();
    let shipped: std::collections::BTreeSet<&str> =
        LEGACY_DEFAULT_DISABLED_APPS.iter().copied().collect();
    if current != shipped {
        return false;
    }
    settings.exceptions.disabled_apps.clear();
    info!(
        retired = LEGACY_DEFAULT_DISABLED_APPS.len(),
        "cleared the default app skip-list from config.toml — PolterType shipped it as a \
         default, it silently disabled auto-switching in every editor and terminal, and it \
         is empty by default as of v0.4.2; add entries back if you want them"
    );
    true
}
