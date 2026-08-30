//! Pane / message / draft-kind enums for the Settings UI.

use std::path::PathBuf;

use iced::widget::text_editor;
use poltertype_core::engine::{ModRole, ModSet};
use poltertype_core::plugins::SettingValue;
use poltertype_core::settings::TrayIconStyle;
use poltertype_layout::LayoutId;

use super::plugin_pane::Slot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// Permission walkthrough. First in the list and first on open
    /// when the tray launches us because the keyboard hooks failed —
    /// at that moment nothing else in this window matters.
    Setup,
    Languages,
    Hotkeys,
    Commands,
    Wordlists,
    General,
    Exceptions,
    Suggestions,
    Plugins,
    About,
}

/// Action kind picker in the "Add command" form. Maps 1:1 to
/// [`poltertype_core::commands::CommandAction`] variants but as a Copy enum
/// so it can drive radio-button state without holding the action's
/// payload (which lives in `command_draft_param` until Add).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandActionKind {
    TypeText,
    SwitchLayout,
    OpenPath,
}

impl CommandActionKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            CommandActionKind::TypeText => "Type text (snippet)",
            CommandActionKind::SwitchLayout => "Switch layout",
            CommandActionKind::OpenPath => "Open file / URL",
        }
    }

    pub(super) fn placeholder(self) -> &'static str {
        match self {
            CommandActionKind::TypeText => "Best regards,\\nDmytro",
            CommandActionKind::SwitchLayout => "en-US",
            CommandActionKind::OpenPath => "https://… or C:\\path\\to\\file.md",
        }
    }
}

/// Which user-overlay file the Wordlists pane is editing. Both live
/// under `<config-dir>/poltertype/wordlists/`: [`WordlistKind::Extras`]
/// is `<stem>.txt`, merged into the layout's `user_overlay`, and
/// [`WordlistKind::Stop`] is `<stem>-stop.txt`, merged into its
/// short-stop list. Identical syntax (see
/// [`poltertype_core::layouts::parse_wordlist`]); only the role at
/// engine load time differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordlistKind {
    Extras,
    Stop,
}

impl WordlistKind {
    pub(super) fn suffix(self) -> &'static str {
        match self {
            WordlistKind::Extras => "",
            WordlistKind::Stop => "-stop",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            WordlistKind::Extras => "Extras (full words)",
            WordlistKind::Stop => "Stop list (short tokens)",
        }
    }
}

/// Which hotkey is being rebound right now. `None` = not in capture
/// mode. Stored on the app state so the keyboard subscription can
/// route the next combo to the right setting field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyKind {
    Pause,
    SwitchLast,
}

/// The window's colour-theme preference, persisted as
/// `[general].ui_theme` in `config.toml`. `System` follows the OS
/// light/dark setting (detected once at window start).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    /// Display order of the segmented picker on the General pane.
    pub(super) const ALL: [ThemeChoice; 3] =
        [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    /// Parse the `config.toml` value. Unknown strings fall back to
    /// `System` — same forgiving posture the rest of the settings
    /// schema takes towards hand-edited configs.
    pub(super) fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => ThemeChoice::Light,
            "dark" => ThemeChoice::Dark,
            _ => ThemeChoice::System,
        }
    }

    /// The canonical `config.toml` spelling.
    pub(super) fn config_value(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SelectPane(Pane),

    // ── Plug-ins pane ──────────────────────────────────────────────
    // Addressed by (plug-in index, control index) rather than by key:
    // the manifest is the only thing that says which key a control
    // writes, and a message carrying its own key would let the UI
    // write somewhere the manifest never declared.
    PluginToggled(usize, usize, bool),
    PluginChoiceSelected(usize, usize, String),
    PluginTextChanged(usize, usize, String),
    // ── Repeating groups ───────────────────────────────────────────
    // (plug-in, control, row, field name, value). The field is named
    // rather than indexed because a row is a table; the control index
    // still says which group, so a message cannot write into a key the
    // manifest never declared.
    PluginRecordChanged(usize, usize, usize, String, SettingValue),
    /// Text being typed into a record's field. Written to the file when
    /// something settles, not per keystroke.
    PluginRecordTyped(usize, usize, usize, String, String),
    PluginRecordAdded(usize, usize),
    PluginRecordRemoved(usize, usize, usize),
    /// A button on one card of a repeating group: the plug-in, the
    /// control, the row, and the id of the command to run. The row's
    /// name is read out of the row itself (the manifest says which
    /// field names it), never invented by the pane.
    PluginRecordAction(usize, usize, usize, String),
    /// That button's command finished, and what it printed — shown as
    /// the plug-in's status line, in the plug-in's own words.
    PluginRecordActionDone(usize, String, Result<String, String>),
    /// Open a link a plug-in put beside one of its choices.
    ///
    /// Carries the address rather than a `&'static str` like
    /// [`Self::OpenUrl`], because this one comes out of a manifest —
    /// which is why it is checked before opening: `https` only, and the
    /// pane shows the address as the link text, so what is clicked is
    /// what was read.
    PluginOpenLink(String),
    /// Runs one of the plug-in's declared commands by id.
    PluginCommandClicked(usize, String),
    /// Ask a plug-in to run a control's command again — the pane's
    /// refresh button, and what opening the pane sends for each
    /// command-backed control it has not asked for yet.
    PluginOutputRefresh(usize, Slot),
    /// A command answered, for every control that asked for it. `Err`
    /// is shown rather than swallowed: a plug-in that cannot answer is
    /// something the user should see.
    ///
    /// Several controls, because two may share one command — asking the
    /// same question twice means reading a chat client's sidebar twice.
    PluginOutputLoaded(usize, Vec<Slot>, Result<String, String>),
    /// One of a suggestion box's candidates was picked: the plug-in, the
    /// box, and the value — which is the row's id, not its label.
    PluginSuggestPicked(usize, Slot, String),
    /// Show, or stop showing, what a box has to suggest. Typing opens
    /// the list on its own; this opens and closes it without typing.
    PluginSuggestToggled(usize, Slot),
    /// A row of a list control was ticked or unticked: add or remove
    /// that name in the plug-in's own config array.
    PluginListToggled(usize, usize, String, bool),
    /// Tick (or untick) every row a list control is showing.
    PluginListAll(usize, usize, bool),
    /// A section was chosen in the plug-in's own navigation list.
    PluginSectionSelected(usize, usize),
    LanguageToggled(LayoutId, bool),
    LanguageIgnoreToggled(LayoutId, bool),
    AutostartToggled(bool),
    SoundOnCorrectToggled(bool),
    ShowNotificationsToggled(bool),
    SuppressInIdentifiersToggled(bool),
    IdleTimeoutDelta(i32),
    /// `[updates].enabled` — the master switch on the only network
    /// access the app performs.
    AutoUpdateToggled(bool),
    /// `[updates].check_interval_hours`, stepped by the ± buttons.
    UpdateIntervalDelta(i64),

    // ── Hotkeys pane ───────────────────────────────────────────────
    /// Enter capture mode for `kind` (button click → "Press a
    /// combination…").
    HotkeyRebindStart(HotkeyKind),
    /// A complete `<mods>+<key>` combo arrived from the keyboard
    /// subscription while in capture mode.
    HotkeyCaptured(String),
    /// A modifier key went down or came back up while in capture mode.
    /// `held` is what iced says is down *besides* `role`, so a window
    /// that lost focus mid-gesture cannot leave the capture stuck
    /// waiting for a release that already happened.
    HotkeyModifier {
        role: ModRole,
        pressed: bool,
        held: ModSet,
    },
    HotkeyRebindCancel,

    // ── Exceptions pane ────────────────────────────────────────────
    ExceptionDraftChanged(String),
    ExceptionAdd,
    ExceptionRemove(usize),

    // ── Commands pane ──────────────────────────────────────────────
    CommandDraftNameChanged(String),
    /// The typed token, e.g. `anrl` or `((en))`.
    CommandDraftTriggerChanged(String),
    /// Clears the param field: different actions take wildly different
    /// content.
    CommandDraftActionKindChanged(CommandActionKind),
    CommandDraftParamChanged(String),
    /// Apps filter, comma-separated.
    CommandDraftAppsChanged(String),
    /// Validates the draft and clears the form on success.
    CommandAdd,
    CommandRemove(usize),

    // ── Wordlists pane ─────────────────────────────────────────────
    /// Empty string = the global overlay; non-empty = an id from the
    /// configured `[[wordlists.profiles]]`. Load-or-empty like
    /// `WordlistLayoutSelected`.
    WordlistProfileSelected(String),
    /// Loads `<stem><suffix>.txt` into the editor, or empty if missing.
    WordlistLayoutSelected(LayoutId),
    /// Same load-or-empty semantics as `WordlistLayoutSelected`.
    WordlistKindSelected(WordlistKind),
    /// Passed straight through to `text_editor::Content::perform`.
    WordlistEdit(text_editor::Action),

    // ── Suggestions pane ───────────────────────────────────────────
    /// `[suggestions].enabled` — master switch for the typo tooltip.
    SuggestionsToggled(bool),
    /// `[suggestions].max_suggestions`, stepped by the ± buttons.
    SuggestionMaxDelta(i64),
    /// `[suggestions].tooltip_timeout_secs`, stepped by the ± buttons.
    SuggestionTimeoutDelta(i64),
    /// `[suggestions].accept_modifiers` text input. Stored verbatim:
    /// the pane hints inline about strings that disable the chord
    /// instead of rejecting keystrokes, so typos can be fixed in place.
    SuggestionModifiersChanged(String),

    /// Segmented theme picker on the General pane. Applies to the
    /// window immediately; persisted via the normal footer Save.
    ThemeChoiceChanged(ThemeChoice),

    /// Segmented tray-icon picker beside it. Nothing to apply here —
    /// the tray is another process, and it re-reads `config.toml`.
    TrayIconChoiceChanged(TrayIconStyle),

    /// Automatic or manual-only conversion — the same `[general].paused`
    /// the tray's pause item writes, named here as the mode it is
    /// (issue #51). `true` means manual only.
    ManualOnlyChosen(bool),

    ResetDefaults,
    Save,
    /// Reverts the staged edits back to the on-disk values.
    Reload,
    OpenConfigFile,
    OpenLogsDir,
    OpenWordlistsDir,
    OpenLayoutsDir,
    /// Open `url` in the default browser — the About pane's links.
    OpenUrl(&'static str),

    // ── Setup pane ─────────────────────────────────────────────────
    /// Re-run the permission probe. The user goes away, changes
    /// something and comes back, so every answer the pane shows is a
    /// fresh reading, never a cached one.
    SetupRecheck,
    /// Open a URL the probe supplied — a documentation page, or a
    /// macOS `x-apple.systempreferences:` deep link. Owned `String`
    /// rather than `&'static str` because the probe builds these.
    SetupOpen(String),
    /// Selection conversion on or off. Only reachable where the
    /// session can do it — see `SettingsApp::selection_support`.
    SelectionEnabledToggled(bool),
    /// Copy a shell command to the clipboard. We never run it: the
    /// Linux setup script needs `sudo`, and the user reading it first
    /// is the point.
    SetupCopy(String),
    /// Ask the OS for a permission — macOS only, and always the
    /// system's own dialog.
    SetupRequestPermission(poltertype_input::setup::Permission),
    /// Create/adopt the local signing identity and write its name into
    /// `[updates].local_signing_identity` — the Setup pane answer to
    /// permissions dying on every update.
    SetupLocalSigning,
    /// Quit the main tray process and relaunch the bundle: permissions
    /// are read at startup, and sending the user to hunt Quit in the
    /// tray after every grant was the complaint.
    RestartApp,

    /// Intercepted so an unsaved wordlist edit is auto-saved before the
    /// window closes. Carries the `window::Id` to close the right one.
    WindowCloseRequested(iced::window::Id),
}

/// Result of `SettingsApp::flush_wordlist_to_disk`, split so the caller
/// can pick banner phrasing that matches what happened — silent for
/// "nothing to do", neutral for "saved", loud for failures.
#[derive(Debug, Clone)]
pub enum WordlistFlushOutcome {
    /// Buffer wasn't dirty. Auto-save callers suppress the banner here
    /// so navigation clicks don't spam "Auto-saved.".
    Nothing,
    /// Only reachable via the per-pane Save click before any layout has
    /// been picked: a dirty buffer implies the editor was typed in,
    /// which implies a layout.
    NoLayout,
    Saved(PathBuf),
    /// Disk error — the message carries the I/O error rendering.
    Failed(String),
}
