//! Pane / message / draft-kind enums for the Settings UI.

use std::path::PathBuf;

use iced::widget::text_editor;
use poltertype_layout::LayoutId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Languages,
    Hotkeys,
    Commands,
    Wordlists,
    General,
    Exceptions,
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

/// Which user-overlay file the Wordlists pane is currently editing
/// for the selected layout. Both files live under
/// `<config-dir>/poltertype/wordlists/`:
///
/// * [`WordlistKind::Extras`] → `<stem>.txt` — extra dictionary
///   words that get merged into the layout's `user_overlay` set.
/// * [`WordlistKind::Stop`] → `<stem>-stop.txt` — extra short-stop
///   words (≤2 letters) that get merged into the per-layout
///   short-stop list.
///
/// The two files have identical syntax (one word per line, `#`
/// comments, blank lines ignored — see
/// [`poltertype_core::layouts::parse_wordlist`]); only their semantic role
/// differs at engine load time.
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

#[derive(Debug, Clone)]
pub enum Message {
    SelectPane(Pane),
    LanguageToggled(LayoutId, bool),
    LanguageIgnoreToggled(LayoutId, bool),
    AutostartToggled(bool),
    SoundOnCorrectToggled(bool),
    ShowNotificationsToggled(bool),
    SuppressInIdentifiersToggled(bool),
    IdleTimeoutDelta(i32),

    // ── Hotkeys pane ───────────────────────────────────────────────
    /// Enter capture mode for `kind` (button click → "Press a
    /// combination…").
    HotkeyRebindStart(HotkeyKind),
    /// A complete `<mods>+<key>` combo arrived from the keyboard
    /// subscription while in capture mode.
    HotkeyCaptured(String),
    /// User cancels capture mode without rebinding.
    HotkeyRebindCancel,

    // ── Exceptions pane ────────────────────────────────────────────
    /// Text-input edit on the "add new disabled app" field.
    ExceptionDraftChanged(String),
    /// "Add" button click.
    ExceptionAdd,
    /// "×" button per existing entry.
    ExceptionRemove(usize),

    // ── Commands pane ──────────────────────────────────────────────
    /// "Add command" form: name field changed.
    CommandDraftNameChanged(String),
    /// "Add command" form: trigger text input changed (the typed
    /// token like `anrl` or `((en))`).
    CommandDraftTriggerChanged(String),
    /// "Add command" form: action kind radio changed (TypeText /
    /// SwitchLayout / OpenPath). Clears the param field — different
    /// actions take wildly different content.
    CommandDraftActionKindChanged(CommandActionKind),
    /// "Add command" form: action-specific param field changed.
    CommandDraftParamChanged(String),
    /// "Add command" form: apps filter input (comma-separated).
    CommandDraftAppsChanged(String),
    /// "Add command" form: Add button pressed. Validates and pushes
    /// to `settings.commands`; clears the form on success.
    CommandAdd,
    /// "×" button on an existing command row.
    CommandRemove(usize),

    // ── Wordlists pane ─────────────────────────────────────────────
    /// User clicked one of the profile buttons. Empty string =
    /// global overlay; non-empty = profile id matching one of the
    /// configured `[[wordlists.profiles]]` entries. Same load-or-
    /// empty behaviour as `WordlistLayoutSelected`.
    WordlistProfileSelected(String),
    /// User picked a different layout from the layout row — load
    /// `<stem><suffix>.txt` into the editor (or empty if missing).
    WordlistLayoutSelected(LayoutId),
    /// User flipped between Extras / Stop for the same layout.
    /// Same load-or-empty semantics as `WordlistLayoutSelected`.
    WordlistKindSelected(WordlistKind),
    /// Editor sent us an action (insert / delete / move cursor / …).
    /// We pass it straight through to `text_editor::Content::perform`.
    WordlistEdit(text_editor::Action),

    ResetDefaults,
    Save,
    /// Reverts the staged edits back to the on-disk values.
    Reload,
    OpenConfigFile,
    OpenLogsDir,
    OpenWordlistsDir,
    OpenLayoutsDir,

    /// User clicked the window close button (or otherwise asked
    /// the OS to close the window). We intercept this to auto-save
    /// any unsaved wordlist edit before letting the window close —
    /// see `subscription` and the matching `update` arm for the
    /// rationale. Carries the `window::Id` so we close the right
    /// window in case iced ever multi-windows the Settings UI.
    WindowCloseRequested(iced::window::Id),
}

/// Result of `SettingsApp::flush_wordlist_to_disk`. The variants
/// let the caller pick banner phrasing that matches what actually
/// happened — silent for "nothing to do", neutral for "saved",
/// loud for failures.
#[derive(Debug, Clone)]
pub enum WordlistFlushOutcome {
    /// Buffer wasn't dirty — nothing to save, nothing to report.
    /// Auto-save callers (layout/profile/kind switch) suppress the
    /// banner in this case so the UI doesn't spam "Auto-saved." on
    /// every navigation click.
    Nothing,
    /// No layout selected when the flush was attempted. Only
    /// reachable via the per-pane Save click before any layout
    /// has been picked — auto-save callers always have a layout
    /// because the dirty flag implies the user typed in the editor,
    /// which only opens once a layout is picked.
    NoLayout,
    /// Successful write to the resolved overlay path.
    Saved(PathBuf),
    /// Disk error — message contains the I/O error rendering.
    Failed(String),
}
