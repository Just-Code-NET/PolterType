//! The `config.toml` schema: every settings struct and its defaults.
//! (`default_*` fns live here because serde resolves their paths
//! relative to the structs they annotate.)

use super::*;
use crate::commands::UserCommand;
use crate::wordlist_profiles::WordlistSettings;
use poltertype_types::LayoutId;
use serde::{Deserialize, Serialize};

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
    /// its own subdirectory under `<config-dir>/poltertype/wordlists/profiles/<id>/`
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
pub(crate) fn default_disabled_apps() -> Vec<String> {
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
