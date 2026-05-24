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

use crate::commands::UserCommand;
use crate::wordlist_profiles::WordlistSettings;

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
    /// User-defined "smart commands" — additional `[[commands]]`
    /// hotkey entries beyond the two built-in pause / switch-last
    /// actions in `[hotkeys]`. See [`crate::commands`] for the
    /// schema and the rationale behind keeping the built-in two in
    /// `[hotkeys]` and the rest here.
    #[serde(default)]
    pub commands: Vec<UserCommand>,
    /// Per-application wordlist profiles. Each profile points at
    /// its own subdirectory under `<config-dir>/kb-switcher/wordlists/profiles/<id>/`
    /// and gets activated when the foreground app matches the
    /// profile's `apps` list. See [`crate::wordlist_profiles`].
    #[serde(default)]
    pub wordlists: WordlistSettings,
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
            commands: Vec::new(),
            wordlists: WordlistSettings::default(),
            sounds: SoundSettings::default(),
            ai: AiSettings::default(),
        }
    }
}

/// `#[serde(default)]` on every settings struct: any field missing
/// from the user's `config.toml` falls back to its `Default`. That
/// gives us forward-compat — new fields added in later versions read
/// existing configs without scary parse errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
pub struct EngineSettings {
    pub min_word_length: usize,
    pub confidence_threshold: f32,
    pub ignore_in_password_fields: bool,
    /// Word-buffer idle timeout (ms) — clears the buffer if the user
    /// pauses for this long.
    pub idle_timeout_ms: u64,
    /// Skip auto-switching when the just-typed token looks like a
    /// programming-language identifier (snake_case, camelCase,
    /// letter+digit, …). The manual switch hotkey
    /// (`Ctrl+Shift+Backspace`) bypasses this filter — so users can
    /// still fix wrong-layout identifiers explicitly. Default: on.
    /// See `docs/DECISIONS.md` for the reasoning.
    pub suppress_in_identifiers: bool,
    /// Skip auto-switching when the rendered word is ALL CAPS (held
    /// Shift / Caps Lock throughout, ≥2 letters, every alphabetic
    /// character uppercase). This is the textbook abbreviation case
    /// — `URL`, `HTTP`, `API`, `ССЫЛКА` — where the user typed
    /// deliberately and a layout flip is more disruptive than
    /// helpful. The manual switch hotkey still works on these
    /// buffers (`last_word` is stashed before any filter). Default:
    /// on.
    pub suppress_for_all_caps: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            min_word_length: 3,
            confidence_threshold: 0.55,
            ignore_in_password_fields: true,
            idle_timeout_ms: 2000,
            suppress_in_identifiers: true,
            suppress_for_all_caps: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExceptionSettings {
    /// Foreground apps where auto-switching is disabled. Each entry
    /// is matched case-insensitively against the focused process's
    /// executable basename (e.g. `Code.exe` on Windows, `code` on
    /// Linux, `Code` on macOS). The manual switch hotkey
    /// (`Ctrl+Shift+Backspace`) ignores this list — devs can still
    /// explicitly fix a wrong-layout word inside an IDE.
    ///
    /// Defaults cover the most common modern editors / IDEs /
    /// terminals across all three OSes; users edit `config.toml`
    /// to adjust.
    #[serde(default = "default_disabled_apps")]
    pub disabled_apps: Vec<String>,
    /// Words that should never be auto-corrected.
    #[serde(default)]
    pub word_whitelist: Vec<String>,
}

impl Default for ExceptionSettings {
    fn default() -> Self {
        Self {
            disabled_apps: default_disabled_apps(),
            word_whitelist: Vec::new(),
        }
    }
}

/// Default per-app skip-list. Conservative: we ship the apps where
/// auto-switching is most likely to corrupt syntax. Anything else the
/// user has to add by hand. Matched case-insensitively, basename only.
fn default_disabled_apps() -> Vec<String> {
    [
        // Editors / IDEs (Windows .exe + Linux/macOS bare names).
        "Code.exe",
        "code",
        "Code - Insiders.exe",
        "code-insiders",
        "Cursor.exe",
        "cursor",
        "Cursor",
        "idea64.exe",
        "idea.exe",
        "idea",
        "rustrover64.exe",
        "rustrover",
        "pycharm64.exe",
        "pycharm",
        "webstorm64.exe",
        "webstorm",
        "clion64.exe",
        "clion",
        "goland64.exe",
        "goland",
        "phpstorm64.exe",
        "phpstorm",
        "rider64.exe",
        "rider",
        "datagrip64.exe",
        "datagrip",
        "android-studio.exe",
        "android-studio",
        "fleet.exe",
        "fleet",
        "sublime_text.exe",
        "sublime_text",
        "Sublime Text",
        "Notepad++.exe",
        "Zed.exe",
        "zed",
        "Zed",
        "neovide.exe",
        "neovide",
        "gvim.exe",
        "gvim",
        "nvim-qt.exe",
        "emacs.exe",
        // Terminals (Windows + Linux/macOS).
        "WindowsTerminal.exe",
        "wt.exe",
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
        "ConEmu64.exe",
        "ConEmu.exe",
        "tabby.exe",
        "tabby",
        "alacritty.exe",
        "alacritty",
        "wezterm-gui.exe",
        "wezterm",
        "kitty.exe",
        "kitty",
        "konsole",
        "gnome-terminal",
        "gnome-terminal-server",
        "xterm",
        "tilix",
        "Terminal", // macOS Terminal.app
        "iTerm2",
        // Terminal-hosted shells / multiplexers.
        "git-bash.exe",
        "mintty.exe",
        "tmux",
        "screen",
        // Text-mode editors hosted in terminals — we skip them by
        // window class only loosely; the parent terminal exe already
        // matches above.
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
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
        assert!(s.engine.suppress_in_identifiers);
        assert!(s.engine.suppress_for_all_caps);
    }

    /// Forward-compat regression: a config that's missing a struct
    /// field added after the user wrote the file must still parse —
    /// that's what `#[serde(default)]` on every settings struct buys
    /// us.
    #[test]
    fn old_config_missing_new_field_still_parses() {
        let raw =
            "schema_version = 1\n\n[engine]\nmin_word_length = 4\nconfidence_threshold = 0.7\n";
        let s: Settings = toml::from_str(raw).expect("parse");
        assert_eq!(s.engine.min_word_length, 4);
        // `suppress_in_identifiers` / `suppress_for_all_caps` were
        // missing from the user's file but the defaults kicked in.
        assert!(s.engine.suppress_in_identifiers);
        assert!(s.engine.suppress_for_all_caps);
    }

    /// User commands sit in their own `[[commands]]` table. A full
    /// config block including one must round-trip through the live
    /// `Settings` struct — the regression we care about is that
    /// `CommandsSettings` is wired in correctly (no `serde(skip)`,
    /// no `default` collision dropping the user data on save).
    #[test]
    fn commands_section_round_trips_inside_full_settings() {
        let raw = r#"
schema_version = 1

[[commands]]
id      = "anrl"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }
"#;
        let parsed: Settings = toml::from_str(raw).expect("parse with commands");
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.commands[0].id, "anrl");
        assert_eq!(parsed.commands[0].trigger, "anrl");

        // And the round-trip back to TOML must preserve the entry —
        // a `Default` collision or stray `serde(skip)` would silently
        // drop it on first save, which is the worst kind of bug.
        let serialised = toml::to_string_pretty(&parsed).expect("serialise");
        let back: Settings = toml::from_str(&serialised).expect("parse round-trip");
        assert_eq!(back.commands.len(), 1);
        assert_eq!(back.commands[0].id, "anrl");
        assert_eq!(back.commands[0].trigger, "anrl");
    }

    /// Legacy configs from beta.4 and earlier had no `[[commands]]`
    /// section. They must still parse — the user shouldn't have to
    /// edit their config to keep the app starting.
    #[test]
    fn legacy_config_without_commands_still_parses() {
        let raw = r#"
schema_version = 1

[hotkeys]
pause_toggle = "Ctrl+Shift+Space"
manual_switch_last = "Ctrl+Shift+Backspace"
"#;
        let parsed: Settings = toml::from_str(raw).expect("parse legacy");
        assert!(parsed.commands.is_empty());
        assert_eq!(parsed.hotkeys.pause_toggle, "Ctrl+Shift+Space");
    }

    #[test]
    fn default_disabled_apps_covers_common_editors() {
        let s = Settings::default();
        let lower: Vec<String> = s
            .exceptions
            .disabled_apps
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        for must in ["code.exe", "cursor.exe", "windowsterminal.exe", "alacritty"] {
            assert!(
                lower.iter().any(|s| s == must),
                "expected `{must}` in default disabled_apps"
            );
        }
    }
}
