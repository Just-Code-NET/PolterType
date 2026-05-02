//! Persistent settings stored as TOML in the platform's standard
//! per-user config dir.
//!
//! Path:
//!   * Windows : `%APPDATA%\kb-switcher\config.toml`
//!   * macOS   : `~/Library/Application Support/kb-switcher/config.toml`
//!   * Linux   : `$XDG_CONFIG_HOME/kb-switcher/config.toml`
//!
//! Schema is versioned (`schema_version`) so future migrations can
//! tell old configs apart without breaking the user's file.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use kb_types::LayoutId;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not locate a per-user config directory for kb-switcher")]
    NoConfigDir,
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("toml deserialize: {0}")]
    Deserialize(#[from] toml::de::Error),
    #[error("toml serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
}

// ─── Schema ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub schema_version: u32,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub languages: LanguageSettings,
    #[serde(default)]
    pub engine: EngineSettings,
    #[serde(default)]
    pub exceptions: ExceptionSettings,
    #[serde(default)]
    pub hotkeys: HotkeySettings,
    #[serde(default)]
    pub sounds: SoundSettings,
    /// Reserved for the AI subsystem (Phase 7). Disabled by default.
    #[serde(default)]
    pub ai: AiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            general: GeneralSettings::default(),
            languages: LanguageSettings::default(),
            engine: EngineSettings::default(),
            exceptions: ExceptionSettings::default(),
            hotkeys: HotkeySettings::default(),
            sounds: SoundSettings::default(),
            ai: AiSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralSettings {
    pub autostart: bool,
    pub sound_on_correct: bool,
    pub show_notifications: bool,
    pub ui_language: String,
    pub log_level: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            autostart: true,
            sound_on_correct: true,
            show_notifications: false,
            ui_language: "system".into(),
            log_level: "info".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LanguageSettings {
    /// Layouts the engine considers when deciding. Empty = use every
    /// layout known to the OS.
    #[serde(default)]
    pub active: Vec<LayoutId>,
    /// Layouts the engine should never switch to, even if the OS has
    /// them enabled.
    #[serde(default)]
    pub ignored: Vec<LayoutId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineSettings {
    pub min_word_length: usize,
    pub confidence_threshold: f32,
    pub ignore_in_password_fields: bool,
    /// Word-buffer idle timeout (ms) — clears the buffer if the user
    /// pauses for this long.
    pub idle_timeout_ms: u64,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            min_word_length: 3,
            confidence_threshold: 0.55,
            ignore_in_password_fields: true,
            idle_timeout_ms: 2000,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ExceptionSettings {
    /// Foreground apps where auto-switching is disabled (per-OS exe /
    /// bundle id / window-class match — interpreted by FocusTracker).
    #[serde(default)]
    pub disabled_apps: Vec<String>,
    /// Words that should never be auto-corrected.
    #[serde(default)]
    pub word_whitelist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HotkeySettings {
    pub pause_toggle: String,
    pub manual_switch_last: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            pause_toggle: "Ctrl+Shift+Space".into(),
            manual_switch_last: "Ctrl+Shift+Backspace".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SoundSettings {
    pub theme: String,
    pub volume: f32,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            theme: "default".into(),
            volume: 0.6,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AiSettings {
    pub enabled: bool,
    /// Even when `enabled = true`, network calls remain blocked until
    /// this is also `true`. Two-toggle design, by design.
    pub allow_remote: bool,
}

// ─── SettingsStore ───────────────────────────────────────────────────

/// Loaded settings, swappable at runtime via [`SettingsStore::update`].
/// All readers see a consistent snapshot through `parking_lot::RwLock`.
pub struct SettingsStore {
    path: PathBuf,
    inner: RwLock<Settings>,
}

impl SettingsStore {
    pub fn project_dirs() -> Result<ProjectDirs, SettingsError> {
        ProjectDirs::from("dev", "opensource", "kb-switcher").ok_or(SettingsError::NoConfigDir)
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

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn update<F: FnOnce(&mut Settings)>(&self, f: F) -> Result<(), SettingsError> {
        let mut guard = self.inner.write();
        f(&mut guard);
        write_atomically(&self.path, &guard)
    }
}

fn write_atomically(path: &Path, s: &Settings) -> Result<(), SettingsError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = toml::to_string_pretty(s)?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, serialized)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_serialise_and_round_trip() {
        let s = Settings::default();
        let serialized = toml::to_string_pretty(&s).expect("serialize");
        let back: Settings = toml::from_str(&serialized).expect("parse");
        assert_eq!(s, back);
    }

    #[test]
    fn missing_keys_use_defaults() {
        // Minimal valid TOML — every section uses its `Default::default()`.
        let s: Settings = toml::from_str("schema_version = 1").expect("parse");
        assert_eq!(s.engine.min_word_length, 3);
        assert_eq!(s.general.log_level, "info");
        assert!(!s.ai.enabled);
    }
}
