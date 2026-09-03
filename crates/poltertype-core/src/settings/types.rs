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
    /// User-defined "smart commands" — `[[commands]]` entries beyond
    /// the two built-in actions in `[hotkeys]`. See [`crate::commands`]
    /// for the schema and `docs/ARCHITECTURE.md` for the split.
    #[serde(default)]
    pub commands: Vec<UserCommand>,
    /// Whether `run_shell` smart commands may execute at all.
    ///
    /// **Off by default, and that is the security boundary.** An entry
    /// that runs a program turns a shared or stolen `config.toml` into
    /// code that fires the next time the user types an ordinary word —
    /// see the threat model in [`crate::commands::shell`]. While this
    /// is false the entries still parse and show in Settings; they
    /// refuse to run and say so once per firing.
    #[serde(default)]
    pub commands_allow_run_shell: bool,
    /// Per-application wordlist profiles. Each profile points at
    /// its own subdirectory under `<config-dir>/poltertype/wordlists/profiles/<id>/`
    /// and gets activated when the foreground app matches the
    /// profile's `apps` list. See [`crate::wordlist_profiles`].
    #[serde(default)]
    pub wordlists: WordlistSettings,
    #[serde(default)]
    pub sounds: SoundSettings,
    /// Spelling-suggestion tooltip for mistyped (same-layout) words.
    /// See [`SuggestionSettings`].
    #[serde(default)]
    pub suggestions: SuggestionSettings,
    /// Background update checks against GitHub Releases. The **only**
    /// network access a default build performs — see [`UpdateSettings`].
    #[serde(default)]
    pub updates: UpdateSettings,
    /// Converting a *selected* passage rather than the last word.
    /// **Off by default** — see [`SelectionSettings`].
    #[serde(default)]
    pub selection: SelectionSettings,
    /// Reserved for the AI subsystem (Phase 7). Disabled by default.
    #[serde(default)]
    pub ai: AiSettings,
}

/// Converting whatever the user has selected, with the same hotkey
/// that switches the last word (issue #32).
///
/// **Off by default, and that is the point.** Doing this at all means
/// pressing `Ctrl+C` into somebody else's application and reading
/// their clipboard — a strictly larger reach than the rest of the app
/// has, and one nobody should acquire by upgrading. Turning it on is
/// the user saying which trade they want; leaving it off costs them
/// nothing, because the hotkey behaves exactly as it always did.
///
/// Two things it cannot promise even when on, both recorded in
/// `docs/KNOWN-GAPS.md`: a clipboard holding an image or files reads
/// as empty through a text API, so it is replaced by the selection and
/// not put back; and there is no way to know a password field from any
/// other, so a selection inside one is copied like any other text.
/// `false` by default, which `derive(Default)` gives — and the default
/// is the whole point, so it is spelled out here rather than left to a
/// reader to infer from the type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SelectionSettings {
    pub enabled: bool,
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
            commands_allow_run_shell: false,
            wordlists: WordlistSettings::default(),
            sounds: SoundSettings::default(),
            suggestions: SuggestionSettings::default(),
            updates: UpdateSettings::default(),
            selection: SelectionSettings::default(),
            ai: AiSettings::default(),
        }
    }
}

/// `#[serde(default)]` on every settings struct, so a field added in a
/// later version still reads an existing `config.toml` without a parse
/// error.
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
    /// How the tray icon is drawn: `"color"` (a hue per layout),
    /// `"mono"` (one neutral badge), `"flag"` (the layout's country,
    /// falling back to `"color"` for a country the app has no drawing
    /// for) or `"hidden"` (no tray icon and no tray menu — the
    /// Settings window still opens with `poltertype --settings`).
    /// Read through
    /// [`TrayIconStyle`](crate::settings::TrayIconStyle), which falls
    /// back to `"color"` on anything else.
    pub tray_icon: String,
    pub log_level: String,
    /// Auto-switching is off. Written by the app itself every time the
    /// pause hotkey or the tray item is used, so the state the user
    /// left it in is the state it starts in (issue #46) — and read
    /// back live, so setting it here pauses a running app too.
    pub paused: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            autostart: true,
            sound_on_correct: true,
            show_notifications: false,
            ui_language: "system".into(),
            ui_theme: "system".into(),
            tray_icon: "color".into(),
            log_level: "info".into(),
            paused: false,
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
    /// programming identifier. The manual switch hotkey bypasses this
    /// filter. Default: on; see `docs/DECISIONS.md`.
    pub suppress_in_identifiers: bool,
    /// Skip auto-switching when the rendered word is ALL CAPS (≥2
    /// letters, every cased one uppercase) — the abbreviation case, typed
    /// deliberately. The manual hotkey still works, since `last_word` is
    /// stashed before any filter. Default: on.
    pub suppress_for_all_caps: bool,
    /// Hold the user's keystrokes back while a correction burst is on
    /// the wire, and replay them after it — the deterministic guard
    /// against a letter typed mid-correction landing inside the
    /// corrected word. Costs a small input delay right after each
    /// correction, which is why it defaults off — see the trade in
    /// docs/PERMISSIONS.md. `POLTERTYPE_HOLD_KEYS` (`1`/`0`) overrides
    /// in either direction; the gate is built at startup, so changing
    /// this needs a restart.
    pub hold_keys: bool,
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
            hold_keys: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExceptionSettings {
    /// Foreground apps where auto-switching is disabled, matched
    /// case-insensitively against the focused process's executable
    /// basename (`Code.exe`, `code`, `Code`). The manual switch hotkey
    /// ignores this list.
    ///
    /// **Empty by default: we do not decide for the user where they are
    /// allowed to type.** A shipped skip-list of editors and terminals
    /// armed itself the moment the Linux focus tracker landed and made
    /// the app look broken — see "Reversed: no default app skip-list" in
    /// `docs/DECISIONS.md`. The engine's own guards apply everywhere.
    #[serde(default)]
    pub disabled_apps: Vec<String>,
    /// Words that should never be auto-corrected.
    #[serde(default)]
    pub word_whitelist: Vec<String>,
}

impl ExceptionSettings {
    /// True iff `stripped` — already canonicalised with
    /// [`poltertype_detect::letters_only_lower`] — is on the never-touch
    /// list. Entries are canonicalised the same way on the fly, so a
    /// config line can be spelled naturally (`NGINX`, `just-code.net`,
    /// `ім'я`) and still match the buffer's rendering.
    pub fn is_whitelisted(&self, stripped: &str) -> bool {
        self.word_whitelist
            .iter()
            .any(|w| poltertype_detect::letters_only_lower(w) == stripped)
    }
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
            // Platform-neutral on purpose: macOS needs a different pause
            // chord, but the binary substitutes it off the live backend
            // name. A default varying by build target would make one
            // `config.toml` mean two different things.
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

/// Spelling suggestions for mistyped words.
///
/// A completed word that is neither a wrong-layout word the engine
/// would correct nor in the current dictionary gets nearby dictionary
/// words offered in a small tooltip; a click or the accept chord plus a
/// digit replaces it in place. Purely local computation over the
/// bundled dictionaries — no network, nothing typed leaves RAM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SuggestionSettings {
    /// Master switch. On by default — the tooltip never steals focus
    /// and never touches text by itself, so it is safe to show.
    pub enabled: bool,
    /// Most suggestions ever offered at once. Clamped to 1..=9 at
    /// read time — each entry is addressed by one digit key.
    pub max_suggestions: usize,
    /// Seconds the tooltip stays on screen before hiding itself.
    pub tooltip_timeout_secs: u64,
    /// Modifier half of the keyboard-accept chord: `<modifiers>+1` …
    /// `<modifiers>+9` applies the Nth suggestion while the tooltip is
    /// up. Parsed like `[hotkeys]` strings; empty disables keyboard
    /// accept, leaving click-to-apply only.
    pub accept_modifiers: String,
}

impl Default for SuggestionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_suggestions: 5,
            tooltip_timeout_secs: 30,
            accept_modifiers: "Ctrl+Shift".into(),
        }
    }
}

impl SuggestionSettings {
    /// `max_suggestions` with the 1..=9 digit-addressability clamp.
    pub fn max_clamped(&self) -> usize {
        self.max_suggestions.clamp(1, 9)
    }

    /// Tooltip lifetime with a sane floor (a sub-second tooltip is
    /// unusable) and ceiling (an hour-long tooltip is a leak).
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.tooltip_timeout_secs.clamp(3, 600))
    }
}

/// Automatic updates from GitHub Releases — the one place a default
/// build talks to the network.
///
/// GitHub sees what any HTTP server sees: the connecting IP and a
/// User-Agent naming the running version. No request body, no query
/// string, nothing about the user. **This is not telemetry and it does
/// not become telemetry.** `enabled = false` switches all of it off
/// permanently, with no residual "just one check on startup".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check for, download and stage new releases in the background. On
    /// by default: an opt-in nobody finds leaves users on old builds of
    /// an *unsigned* app, which is the worse security posture.
    pub enabled: bool,
    /// Hours between checks. Clamped to a sane floor at read time
    /// (see [`UpdateSettings::interval`]) so a hand-edited `0` cannot
    /// turn the updater into a request loop against GitHub.
    pub check_interval_hours: u64,
    /// macOS: common name of a codesign identity in the login keychain.
    /// Non-empty, and the updater re-signs every swapped bundle with it,
    /// so TCC keeps the Accessibility / Input Monitoring grants across
    /// updates (they key on certificate + identifier instead of the
    /// bundle hash). Empty — ad-hoc updates as before, and the installer
    /// resets the two stale TCC records instead, so the Ask buttons
    /// work right after the swap. Set up once with
    /// `poltertype --setup-local-signing`.
    pub local_signing_identity: String,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_hours: 24,
            local_signing_identity: String::new(),
        }
    }
}

impl UpdateSettings {
    /// The check interval, with the hand-edit floor applied: a
    /// `check_interval_hours = 0` — a typo, or a user reasoning that
    /// zero means "off" — would hammer GitHub from every installed copy.
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
    /// Detectors to construct when `enabled`. Parsed even in a build
    /// without the `ai` feature — a config file must not stop being
    /// readable because of how the binary was compiled — and simply
    /// ignored there.
    #[serde(default)]
    pub plugins: Vec<poltertype_types::AiPluginConfig>,
}
