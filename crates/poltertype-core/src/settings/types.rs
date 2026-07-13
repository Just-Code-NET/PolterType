//! The `config.toml` schema: every settings struct and its defaults.
//! (`default_*` fns live here because serde resolves their paths
//! relative to the structs they annotate.)

use super::*;
use crate::commands::UserCommand;
use crate::wordlist_profiles::WordlistSettings;
use poltertype_types::LayoutId;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    /// Background update checks against GitHub Releases. The **only**
    /// network access a default build performs — see [`UpdateSettings`].
    #[serde(default)]
    pub updates: UpdateSettings,
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
            updates: UpdateSettings::default(),
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
    /// Colour theme of the Settings window: `"system"` (follow the
    /// OS light/dark preference), `"light"`, or `"dark"`. Unknown
    /// values fall back to `"system"` at read time — same forgiving
    /// posture as `ui_language`.
    pub ui_theme: String,
    pub log_level: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            autostart: true,
            sound_on_correct: true,
            show_notifications: false,
            ui_language: "system".into(),
            ui_theme: "system".into(),
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExceptionSettings {
    /// Foreground apps where auto-switching is disabled. Each entry
    /// is matched case-insensitively against the focused process's
    /// executable basename (e.g. `Code.exe` on Windows, `code` on
    /// Linux, `Code` on macOS). The manual switch hotkey
    /// (`Ctrl+Shift+Backspace`) ignores this list — devs can still
    /// explicitly fix a wrong-layout word inside an IDE.
    ///
    /// **Empty by default: we do not decide for the user where they
    /// are allowed to type.** We used to ship a ~50-entry list of
    /// editors, IDEs and terminals here, on the theory that
    /// auto-switching is most likely to corrupt syntax there. It was
    /// harmless only for as long as it was inert: no Linux focus
    /// tracker existed, `focused_exe()` returned `None`, and the list
    /// never matched. The moment the Hyprland/X11 tracker landed the
    /// list armed itself and the app went silent in exactly the
    /// windows a developer types in — indistinguishable, from the
    /// outside, from "layout switching is broken". A default that only
    /// works because it never runs is not a default; the engine's own
    /// guards (`suppress_in_identifiers`, `min_word_length`,
    /// dictionary confidence) are what keep code safe, and they apply
    /// everywhere. Users who *do* want an app skipped add it to
    /// `config.toml` themselves.
    #[serde(default)]
    pub disabled_apps: Vec<String>,
    /// Words that should never be auto-corrected.
    #[serde(default)]
    pub word_whitelist: Vec<String>,
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

/// Automatic updates from GitHub Releases.
///
/// This is the one place a default PolterType build talks to the
/// network, and it is worth being precise about what that means. With
/// `enabled = true` the app periodically fetches a small JSON manifest
/// from `github.com`, and when it names a newer version, downloads that
/// release's installer for this platform and verifies its checksum. The
/// download is staged, never installed under the user's hands — it is
/// applied when they quit or explicitly ask for a restart.
///
/// GitHub therefore sees what any HTTP server sees: the connecting IP
/// and a User-Agent naming the running version. Nothing about the user,
/// their typing, their layouts or their configuration is transmitted —
/// there is no request body and no query string. This is not telemetry
/// and it does not become telemetry.
///
/// `enabled = false` switches all of it off, permanently and with no
/// residual "just one check on startup". A user who wants a build that
/// never opens a socket sets this and has one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check for, download and stage new releases in the background.
    ///
    /// On by default. The alternative — an opt-in nobody finds — leaves
    /// users on old builds of an *unsigned* app that they would then
    /// have to update by hand, and that is the worse security posture,
    /// not the better one.
    pub enabled: bool,
    /// Hours between checks. Clamped to a sane floor at read time
    /// (see [`UpdateSettings::interval`]) so a hand-edited `0` cannot
    /// turn the updater into a request loop against GitHub.
    pub check_interval_hours: u64,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_hours: 24,
        }
    }
}

/// Never check more often than this, whatever `config.toml` says.
pub const MIN_UPDATE_INTERVAL_HOURS: u64 = 1;

impl UpdateSettings {
    /// The check interval, with the hand-edit floor applied.
    ///
    /// A release ships roughly monthly; the difference between checking
    /// hourly and daily is nil for the user and free for us to refuse.
    /// The floor exists so that a `check_interval_hours = 0` — a typo, or
    /// a user reasoning that zero means "off" — can't hammer GitHub in a
    /// tight loop from every installed copy of the app.
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.check_interval_hours.max(MIN_UPDATE_INTERVAL_HOURS) * 60 * 60)
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
