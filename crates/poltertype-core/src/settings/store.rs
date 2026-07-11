//! `SettingsStore` — load / snapshot / save orchestration.

use super::*;
use directories::ProjectDirs;
use parking_lot::RwLock;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Loaded settings, swappable at runtime via [`SettingsStore::update`].
/// All readers see a consistent snapshot through `parking_lot::RwLock`.
pub struct SettingsStore {
    path: PathBuf,
    inner: RwLock<Settings>,
}

impl SettingsStore {
    pub fn project_dirs() -> Result<ProjectDirs, SettingsError> {
        ProjectDirs::from("dev", "opensource", "poltertype").ok_or(SettingsError::NoConfigDir)
    }

    pub fn default_path() -> Result<PathBuf, SettingsError> {
        let dirs = Self::project_dirs()?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Load from disk. If the file is missing, write defaults and
    /// return them. If the file exists but is unreadable / invalid,
    /// log a warning and fall back to defaults *without* overwriting
    /// the user's file (so they can fix it manually).
    pub fn load_or_default() -> Result<Self, SettingsError> {
        let path = Self::default_path()?;
        let inner = match fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<Settings>(&s) {
                Ok(parsed) => parsed,
                Err(e) => {
                    warn!(?path, err = %e, "config.toml is invalid, using defaults; \
                        the user's file is preserved for them to fix");
                    Settings::default()
                }
            },
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // No config yet — this may be an upgrade from the
                // pre-rebrand kb-switcher install; adopt its config
                // tree before falling back to defaults. The re-entry
                // is bounded: migration returns `true` only when
                // config.toml now exists, so the second call takes
                // the `Ok` branch.
                if migrate_legacy_config(&path) {
                    return Self::load_or_default();
                }
                let s = Settings::default();
                if let Err(e) = write_atomically(&path, &s) {
                    warn!(?path, err = %e, "could not seed config.toml on first launch");
                } else {
                    info!(?path, "wrote default config.toml on first launch");
                }
                s
            }
            Err(e) => return Err(SettingsError::Io(e)),
        };
        Ok(Self {
            path,
            inner: RwLock::new(inner),
        })
    }

    pub fn snapshot(&self) -> Settings {
        self.inner.read().clone()
    }

    /// In-memory store for engine tests — never touches the user's
    /// real config file. `update()` must not be called on it.
    #[cfg(test)]
    pub(crate) fn for_tests(settings: Settings) -> Self {
        Self {
            path: PathBuf::new(),
            inner: RwLock::new(settings),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Re-read the file from disk and replace the in-memory snapshot.
    /// Returns whether the contents actually changed.
    pub fn reload(&self) -> Result<bool, SettingsError> {
        let s = match fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(SettingsError::Io(e)),
        };
        let parsed: Settings = toml::from_str(&s)?;
        let mut g = self.inner.write();
        let changed = *g != parsed;
        *g = parsed;
        Ok(changed)
    }

    /// Path of the directory containing the file logs.
    pub fn log_dir() -> Result<PathBuf, SettingsError> {
        let dirs = Self::project_dirs()?;
        Ok(dirs.data_local_dir().join("logs"))
    }

    pub fn update<F: FnOnce(&mut Settings)>(&self, f: F) -> Result<(), SettingsError> {
        let mut guard = self.inner.write();
        f(&mut guard);
        write_atomically(&self.path, &guard)
    }
}
