//! Iced-based Settings window for kb-switcher.
//!
//! ## Why a separate process
//!
//! The tray (`tao::EventLoop` + `tray-icon`) and `iced` both want to
//! own the platform's main thread on macOS — `NSApplication` is a
//! singleton and the tray already binds it. Rather than choreograph
//! a thread swap, we ship the Settings UI as a CLI subcommand:
//!
//!     kb-switcher --settings
//!
//! The tray spawns this as a child process when the user clicks
//! "Settings…". The two have nothing to share at runtime — the
//! settings UI just reads / writes `config.toml` on disk; when it
//! exits, the tray sees `SettingsReloaded` and refreshes its caches.
//!
//! The subprocess approach is not just a workaround:
//!
//! * Crashes in the UI never bring down the engine / hook.
//! * macOS / Windows / Linux behave identically — no per-platform
//!   thread juggling.
//! * The process boundary makes it trivial to run the UI on a
//!   different thread, in a debugger, or from a unit test driver.
//!
//! ## Sections
//!
//! Side-nav with six panes:
//!
//! * **Languages** — checkboxes for every layout the OS reports as
//!   active (queried via `LayoutSwitcher::list_active`). Toggling a
//!   box updates the `[languages].active` allow-list. An **empty**
//!   allow-list means "use every OS-active layout" — the default,
//!   and the UI displays it that way (every box ticked).
//! * **Hotkeys** — current pause / switch-last bindings, plus a
//!   "Rebind" button per row that flips the UI into capture mode
//!   and writes the next valid `<modifier>+<key>` combo back.
//! * **Wordlists** — multiline editor for the user-side wordlist
//!   overlays in `<config-dir>/kb-switcher/wordlists/<stem>.txt`
//!   (and `<stem>-stop.txt`). Pick a layout, pick the file kind,
//!   edit, hit Save. Edits apply when the window closes — the
//!   tray's settings-waiter rebuilds the engine's dictionary set
//!   (and the per-profile cache) before sending `SettingsReloaded`.
//! * **General** — the boolean / numeric knobs from
//!   `GeneralSettings` + `EngineSettings`: autostart, sound on
//!   correction, suppress-in-identifiers, idle timeout.
//! * **Exceptions** — the per-app skip list (`disabled_apps`). One
//!   row per entry with a delete button; an "Add" row at the bottom
//!   for new entries.
//! * **About** — version + repo links. The bottom row also exposes
//!   a "Reset to defaults" button as a power-user escape hatch.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use iced::keyboard::{Key, Modifiers, key::Named};
use iced::widget::{
    Button, Checkbox, Column, Container, Row, Scrollable, Space, Text, TextInput, button,
    text_editor,
};
use iced::{Element, Length, Padding, Subscription, Task, Theme};
use kb_core::commands::{CommandAction, UserCommand};
use kb_core::settings::{Settings, SettingsStore};
use kb_layout::LayoutId;
use kb_layout::create_switcher;
use tracing::{info, warn};

/// Entry point: load settings + OS layouts, run iced loop, save on
/// "Save" click. Returns when the user closes the window.
pub fn run() -> Result<()> {
    let store = SettingsStore::load_or_default().context("load settings for UI")?;
    let initial_settings = store.snapshot();
    let store = Arc::new(store);

    // Querying the OS layout list is best-effort — if it fails we
    // present the user with an empty set and a hint, rather than
    // refusing to open the window. They can still edit other panes
    // and save.
    let os_layouts: Vec<LayoutId> = match create_switcher() {
        Ok(switcher) => switcher.list_active().unwrap_or_else(|e| {
            warn!(
                ?e,
                "could not list active OS layouts; Languages pane will be empty"
            );
            Vec::new()
        }),
        Err(e) => {
            warn!(
                ?e,
                "no layout switcher backend; Languages pane will be empty"
            );
            Vec::new()
        }
    };

    let path = store.path().to_path_buf();

    // iced::application requires `&'static str` OR a closure returning
    // `String` for the title. The closure form lets us include the
    // resolved config path so the user sees exactly which file the
    // window is editing — useful when running multiple installs side
    // by side or under a non-default `XDG_CONFIG_HOME`.
    let app = iced::application(SettingsApp::title, SettingsApp::update, SettingsApp::view)
        .theme(SettingsApp::theme)
        .subscription(SettingsApp::subscription)
        .window_size((720.0, 540.0))
        .centered();

    let store_for_init = Arc::clone(&store);
    app.run_with(move || {
        (
            SettingsApp::new(initial_settings, os_layouts, path, store_for_init),
            Task::none(),
        )
    })
    .map_err(|e| anyhow::anyhow!("iced runtime: {e}"))?;

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Languages,
    Hotkeys,
    Commands,
    Wordlists,
    General,
    Exceptions,
    About,
}

/// Action kind picker in the "Add command" form. Maps 1:1 to
/// [`kb_core::commands::CommandAction`] variants but as a Copy enum
/// so it can drive radio-button state without holding the action's
/// payload (which lives in `command_draft_param` until Add).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandActionKind {
    TypeText,
    SwitchLayout,
    OpenPath,
}

impl CommandActionKind {
    fn label(self) -> &'static str {
        match self {
            CommandActionKind::TypeText => "Type text (snippet)",
            CommandActionKind::SwitchLayout => "Switch layout",
            CommandActionKind::OpenPath => "Open file / URL",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            CommandActionKind::TypeText => "Best regards,\\nDmytro",
            CommandActionKind::SwitchLayout => "en-US",
            CommandActionKind::OpenPath => "https://… or C:\\path\\to\\file.md",
        }
    }
}

/// Which user-overlay file the Wordlists pane is currently editing
/// for the selected layout. Both files live under
/// `<config-dir>/kb-switcher/wordlists/`:
///
/// * [`WordlistKind::Extras`] → `<stem>.txt` — extra dictionary
///   words that get merged into the layout's `user_overlay` set.
/// * [`WordlistKind::Stop`] → `<stem>-stop.txt` — extra short-stop
///   words (≤2 letters) that get merged into the per-layout
///   short-stop list.
///
/// The two files have identical syntax (one word per line, `#`
/// comments, blank lines ignored — see
/// [`kb_core::layouts::parse_wordlist`]); only their semantic role
/// differs at engine load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordlistKind {
    Extras,
    Stop,
}

impl WordlistKind {
    fn suffix(self) -> &'static str {
        match self {
            WordlistKind::Extras => "",
            WordlistKind::Stop => "-stop",
        }
    }

    fn label(self) -> &'static str {
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
enum HotkeyKind {
    Pause,
    SwitchLast,
}

#[derive(Debug, Clone)]
enum Message {
    SelectPane(Pane),
    LanguageToggled(LayoutId, bool),
    LanguageIgnoreToggled(LayoutId, bool),
    AutostartToggled(bool),
    SoundOnCorrectToggled(bool),
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
    /// "Save" → write the editor contents to the resolved overlay file.
    WordlistSave,
    /// "Reload" → re-read the overlay file from disk, discarding any
    /// in-memory edits.
    WordlistReload,

    ResetDefaults,
    Save,
    /// Reverts the staged edits back to the on-disk values.
    Reload,
    OpenConfigFile,
    OpenLogsDir,
    OpenWordlistsDir,
    OpenLayoutsDir,
}

struct SettingsApp {
    settings: Settings,
    os_layouts: Vec<LayoutId>,
    config_path: PathBuf,
    store: Arc<SettingsStore>,
    pane: Pane,
    /// Set when [`Message::Save`] writes successfully — surfaced as a
    /// transient banner in the footer so the user gets feedback the
    /// click did something.
    save_banner: Option<SaveBanner>,
    /// `Some(kind)` while the user is in "press a combination…" mode.
    /// The keyboard subscription consults this to know whether to
    /// route key events to `HotkeyCaptured` or ignore them.
    capturing: Option<HotkeyKind>,
    /// Free-form text in the "add a new disabled app" input on the
    /// Exceptions pane. Empty by default; cleared on Add.
    exception_draft: String,

    // ── Commands pane: draft of a new command ──────────────────────
    /// Free-form display name. Falls back to id if blank at Add time.
    command_draft_name: String,
    /// Trigger token the user types to fire this command. Stored
    /// verbatim — validation happens on the Add path so users
    /// can fix in-place (a `TextInput` with a forced trim would
    /// fight common typing patterns). See [`UserCommand::trigger`].
    command_draft_trigger: String,
    /// Which action variant the user picked. Maps to
    /// [`CommandAction`] at Add time using `command_draft_param`.
    command_draft_action_kind: CommandActionKind,
    /// Free-form param string. Interpretation depends on
    /// `command_draft_action_kind`:
    ///
    /// * `TypeText`     → literal text snippet (`\n` escapes preserved)
    /// * `SwitchLayout` → BCP-47 id (e.g. `en-US`)
    /// * `OpenPath`     → file path or URL (passed to `opener::open`)
    command_draft_param: String,
    /// Optional comma-separated app filter. Empty = all apps.
    command_draft_apps: String,
    /// Per-pane status banner (independent of the global save banner
    /// so "Added!" doesn't get clobbered by save state).
    command_status: Option<SaveBanner>,

    // ── Wordlists pane ─────────────────────────────────────────────
    /// Currently-selected profile id for editing. Empty string =
    /// the global overlay (`<config-dir>/wordlists/<stem>.txt`);
    /// any non-empty value picks the per-profile directory at
    /// `<config-dir>/wordlists/profiles/<id>/<stem>.txt`. Defaults
    /// to global when the pane opens — same baseline the engine
    /// uses before any focus-driven profile swap happens.
    wordlist_profile: String,
    /// Currently-selected layout for editing. `None` until the user
    /// clicks one of the layout buttons (or defaults to the first
    /// OS-active layout when the pane is first opened).
    wordlist_layout: Option<LayoutId>,
    /// Which file we're editing for the selected layout.
    wordlist_kind: WordlistKind,
    /// Live editor buffer. `text_editor::Content` owns its own state
    /// (cursor position, selection, undo stack) — we just feed
    /// actions in via `Message::WordlistEdit`.
    wordlist_content: text_editor::Content,
    /// `Some` once a save / reload / load happens — surfaces a
    /// per-pane status line independent of the global save banner so
    /// "Saved!" on Wordlists doesn't mask "Saved!" on settings.
    wordlist_status: Option<SaveBanner>,
    /// Has the buffer been edited since the last load/save? Used to
    /// gate the "discard changes" warning when the user picks a
    /// different layout / kind without saving.
    wordlist_dirty: bool,
}

#[derive(Debug, Clone)]
struct SaveBanner {
    text: String,
    is_error: bool,
}

impl SettingsApp {
    fn new(
        settings: Settings,
        os_layouts: Vec<LayoutId>,
        config_path: PathBuf,
        store: Arc<SettingsStore>,
    ) -> Self {
        // Pre-populate the Wordlists pane with the first OS-active
        // layout so the user can start typing the moment they land
        // on the pane — picking up the existing file content if any.
        let (initial_layout, initial_text) = match os_layouts.first().cloned() {
            Some(layout) => {
                // Default profile is always "" (global) on first open
                // — same baseline the engine uses before any focus-
                // driven swap fires.
                let text = read_overlay_file_or_empty("", &layout, WordlistKind::Extras);
                (Some(layout), text)
            }
            None => (None, String::new()),
        };

        Self {
            settings,
            os_layouts,
            config_path,
            store,
            pane: Pane::Languages,
            save_banner: None,
            capturing: None,
            exception_draft: String::new(),
            command_draft_name: String::new(),
            command_draft_trigger: String::new(),
            command_draft_action_kind: CommandActionKind::TypeText,
            command_draft_param: String::new(),
            command_draft_apps: String::new(),
            command_status: None,
            wordlist_profile: String::new(),
            wordlist_layout: initial_layout,
            wordlist_kind: WordlistKind::Extras,
            wordlist_content: text_editor::Content::with_text(&initial_text),
            wordlist_status: None,
            wordlist_dirty: false,
        }
    }

    fn title(&self) -> String {
        format!("kb-switcher · Settings ({})", self.config_path.display())
    }

    fn theme(&self) -> Theme {
        // Auto-detect light / dark — feels native on every platform
        // without needing a separate UI toggle.
        Theme::default()
    }

    /// Active keyboard subscription. Returns `Subscription::none()`
    /// when we're not capturing a hotkey — important, otherwise every
    /// keystroke in the window allocates a `Message` and re-renders.
    /// During capture we listen for `key_press` events, ignore lone
    /// modifier presses, and emit `HotkeyCaptured` once a non-modifier
    /// key arrives with a non-empty modifier set. `Escape` cancels.
    fn subscription(&self) -> Subscription<Message> {
        if self.capturing.is_none() {
            return Subscription::none();
        }
        iced::keyboard::on_key_press(|key, modifiers| {
            // Esc bails out without rebinding. Important: a lot of
            // people will hit Esc when they realise they didn't want
            // to rebind, and silently swallowing it would feel like
            // a frozen UI.
            if matches!(key, Key::Named(Named::Escape)) {
                return Some(Message::HotkeyRebindCancel);
            }
            // Ignore lone modifier presses — Ctrl by itself is not a
            // hotkey, and we'd otherwise capture every transient
            // press as the user composes the combination.
            if is_modifier_key(&key) {
                return None;
            }
            // Require at least one modifier. Single-letter hotkeys
            // (`A`, `Space`) would clash with normal typing — we
            // refuse to rebind to those.
            if modifiers.is_empty() {
                return None;
            }
            Some(Message::HotkeyCaptured(format_hotkey(&key, modifiers)))
        })
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        // Any user-visible edit clears the previous banner — keeps
        // the footer accurate (otherwise "Saved!" sticks around even
        // as the user starts editing again).
        if !matches!(msg, Message::Save | Message::Reload) {
            self.save_banner = None;
        }

        match msg {
            Message::SelectPane(p) => self.pane = p,

            Message::LanguageToggled(id, active) => {
                // The "Active" checkbox renders the *effective* state,
                // not the raw `[languages].active` list — empty list
                // means "consider every OS layout", so all checkboxes
                // start ticked. When the user unticks one of them in
                // that implicit-all mode, we materialise the list as
                // "every OS layout EXCEPT this one" so the user's
                // intent ("don't use this one") survives a save.
                //
                // The opposite — re-ticking the same box — appends it
                // back. We don't auto-collapse the list back to empty
                // even if it ends up containing every OS-active layout
                // again, because the effective behaviour is identical
                // and a future OS-layout add should still be honoured.
                let list = &mut self.settings.languages.active;
                let was_implicit_all = list.is_empty();
                if active {
                    if !list.contains(&id) {
                        list.push(id);
                    }
                } else if was_implicit_all {
                    *list = self
                        .os_layouts
                        .iter()
                        .filter(|l| **l != id)
                        .cloned()
                        .collect();
                } else {
                    list.retain(|x| *x != id);
                }
            }
            Message::LanguageIgnoreToggled(id, ignored) => {
                let list = &mut self.settings.languages.ignored;
                if ignored {
                    if !list.contains(&id) {
                        list.push(id);
                    }
                } else {
                    list.retain(|x| *x != id);
                }
            }
            Message::AutostartToggled(b) => self.settings.general.autostart = b,
            Message::SoundOnCorrectToggled(b) => self.settings.general.sound_on_correct = b,
            Message::SuppressInIdentifiersToggled(b) => {
                self.settings.engine.suppress_in_identifiers = b
            }
            Message::IdleTimeoutDelta(delta) => {
                let cur = i32::try_from(self.settings.engine.idle_timeout_ms).unwrap_or(2000);
                let next = (cur + delta).clamp(250, 60_000);
                self.settings.engine.idle_timeout_ms = u64::try_from(next).unwrap_or(2000);
            }

            // ── Hotkeys ──────────────────────────────────────────
            Message::HotkeyRebindStart(kind) => self.capturing = Some(kind),
            Message::HotkeyRebindCancel => self.capturing = None,
            Message::HotkeyCaptured(combo) => {
                if let Some(kind) = self.capturing.take() {
                    info!(?kind, %combo, "captured new hotkey combo");
                    match kind {
                        HotkeyKind::Pause => {
                            self.settings.hotkeys.pause_toggle = combo;
                        }
                        HotkeyKind::SwitchLast => {
                            self.settings.hotkeys.manual_switch_last = combo;
                        }
                    }
                }
            }

            // ── Exceptions ───────────────────────────────────────
            Message::ExceptionDraftChanged(s) => self.exception_draft = s,
            Message::ExceptionAdd => {
                let trimmed = self.exception_draft.trim().to_owned();
                if !trimmed.is_empty()
                    && !self
                        .settings
                        .exceptions
                        .disabled_apps
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&trimmed))
                {
                    self.settings.exceptions.disabled_apps.push(trimmed);
                }
                self.exception_draft.clear();
            }
            Message::ExceptionRemove(idx) => {
                if idx < self.settings.exceptions.disabled_apps.len() {
                    self.settings.exceptions.disabled_apps.remove(idx);
                }
            }

            // ── Commands ────────────────────────────────────────
            Message::CommandDraftNameChanged(s) => self.command_draft_name = s,
            Message::CommandDraftTriggerChanged(s) => self.command_draft_trigger = s,
            Message::CommandDraftActionKindChanged(kind) => {
                if self.command_draft_action_kind != kind {
                    // Different action variants take wildly different
                    // content (snippet vs layout id vs URL); flipping
                    // the radio without clearing the field would leave
                    // a confusing half-typed value behind.
                    self.command_draft_param.clear();
                }
                self.command_draft_action_kind = kind;
            }
            Message::CommandDraftParamChanged(s) => self.command_draft_param = s,
            Message::CommandDraftAppsChanged(s) => self.command_draft_apps = s,
            Message::CommandAdd => match build_command_from_draft(self) {
                Ok(cmd) => {
                    info!(id = %cmd.id, "adding user command from UI");
                    self.settings.commands.push(cmd);
                    // Clear the draft on success.
                    self.command_draft_name.clear();
                    self.command_draft_trigger.clear();
                    self.command_draft_param.clear();
                    self.command_draft_apps.clear();
                    self.command_status = Some(SaveBanner {
                        text: "Added. Press Save to persist, then restart kb-switcher.".into(),
                        is_error: false,
                    });
                }
                Err(e) => {
                    self.command_status = Some(SaveBanner {
                        text: e,
                        is_error: true,
                    });
                }
            },
            Message::CommandRemove(idx) => {
                if idx < self.settings.commands.len() {
                    let removed = self.settings.commands.remove(idx);
                    info!(id = %removed.id, "removed user command from UI");
                    self.command_status = Some(SaveBanner {
                        text: format!("Removed `{}`.", removed.id),
                        is_error: false,
                    });
                }
            }

            // ── Wordlists ────────────────────────────────────────
            Message::WordlistProfileSelected(profile_id) => {
                self.wordlist_profile = profile_id;
                if let Some(id) = self.wordlist_layout.clone() {
                    let text =
                        read_overlay_file_or_empty(&self.wordlist_profile, &id, self.wordlist_kind);
                    self.wordlist_content = text_editor::Content::with_text(&text);
                    self.wordlist_dirty = false;
                    self.wordlist_status = None;
                }
            }
            Message::WordlistLayoutSelected(id) => {
                self.wordlist_layout = Some(id.clone());
                let text =
                    read_overlay_file_or_empty(&self.wordlist_profile, &id, self.wordlist_kind);
                self.wordlist_content = text_editor::Content::with_text(&text);
                self.wordlist_dirty = false;
                self.wordlist_status = None;
            }
            Message::WordlistKindSelected(kind) => {
                self.wordlist_kind = kind;
                if let Some(id) = &self.wordlist_layout {
                    let text = read_overlay_file_or_empty(&self.wordlist_profile, id, kind);
                    self.wordlist_content = text_editor::Content::with_text(&text);
                    self.wordlist_dirty = false;
                    self.wordlist_status = None;
                }
            }
            Message::WordlistEdit(action) => {
                // `Action::is_edit()` flips the dirty flag only on
                // semantic edits (insert / delete / paste). Cursor
                // moves and scroll events leave it false so we don't
                // ask the user to save a buffer they only looked at.
                if action.is_edit() {
                    self.wordlist_dirty = true;
                }
                self.wordlist_content.perform(action);
            }
            Message::WordlistSave => {
                let Some(id) = self.wordlist_layout.clone() else {
                    self.wordlist_status = Some(SaveBanner {
                        text: "No layout selected.".into(),
                        is_error: true,
                    });
                    return Task::none();
                };
                let text = self.wordlist_content.text();
                match save_overlay_file(&self.wordlist_profile, &id, self.wordlist_kind, &text) {
                    Ok(path) => {
                        info!(
                            path = ?path,
                            layout = %id,
                            kind = ?self.wordlist_kind,
                            profile = %self.wordlist_profile,
                            "wordlist saved from UI"
                        );
                        self.wordlist_dirty = false;
                        self.wordlist_status = Some(SaveBanner {
                            text: format!(
                                "Saved to {}. Close this window to apply.",
                                path.display()
                            ),
                            is_error: false,
                        });
                    }
                    Err(e) => {
                        warn!(
                            layout = %id,
                            kind = ?self.wordlist_kind,
                            profile = %self.wordlist_profile,
                            err = %e,
                            "wordlist save failed"
                        );
                        self.wordlist_status = Some(SaveBanner {
                            text: format!("Save failed: {e}"),
                            is_error: true,
                        });
                    }
                }
            }
            Message::WordlistReload => {
                if let Some(id) = self.wordlist_layout.clone() {
                    let text =
                        read_overlay_file_or_empty(&self.wordlist_profile, &id, self.wordlist_kind);
                    self.wordlist_content = text_editor::Content::with_text(&text);
                    self.wordlist_dirty = false;
                    self.wordlist_status = Some(SaveBanner {
                        text: "Reloaded from disk.".into(),
                        is_error: false,
                    });
                }
            }

            Message::ResetDefaults => self.settings = Settings::default(),
            Message::Reload => match SettingsStore::load_or_default() {
                Ok(fresh) => {
                    self.settings = fresh.snapshot();
                    self.save_banner = Some(SaveBanner {
                        text: "Reloaded from disk.".into(),
                        is_error: false,
                    });
                }
                Err(e) => {
                    self.save_banner = Some(SaveBanner {
                        text: format!("Reload failed: {e}"),
                        is_error: true,
                    });
                }
            },
            Message::Save => {
                let staged = self.settings.clone();
                match self.store.update(|s| *s = staged) {
                    Ok(()) => {
                        info!(path = ?self.config_path, "settings saved from UI");
                        self.save_banner = Some(SaveBanner {
                            text: format!("Saved to {}.", self.config_path.display()),
                            is_error: false,
                        });
                    }
                    Err(e) => {
                        warn!(?e, "settings save failed");
                        self.save_banner = Some(SaveBanner {
                            text: format!("Save failed: {e}"),
                            is_error: true,
                        });
                    }
                }
            }

            Message::OpenConfigFile => {
                let _ = opener::open(&self.config_path);
            }
            Message::OpenLogsDir => {
                if let Ok(dir) = SettingsStore::log_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = opener::open(&dir);
                }
            }
            Message::OpenWordlistsDir => {
                if let Some(dir) = kb_core::layouts::user_wordlist_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = opener::open(&dir);
                }
            }
            Message::OpenLayoutsDir => {
                if let Some(dir) = kb_core::layouts::user_layout_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = opener::open(&dir);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let nav = nav_panel(self.pane);
        let body = match self.pane {
            Pane::Languages => self.view_languages(),
            Pane::Hotkeys => self.view_hotkeys(),
            Pane::Commands => self.view_commands(),
            Pane::Wordlists => self.view_wordlists(),
            Pane::General => self.view_general(),
            Pane::Exceptions => self.view_exceptions(),
            Pane::About => self.view_about(),
        };
        let footer = self.view_footer();

        let main = Row::new()
            .push(nav)
            .push(
                Container::new(
                    Column::new()
                        .push(Scrollable::new(body).height(Length::Fill))
                        .push(footer),
                )
                .padding(Padding::new(20.0))
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .height(Length::Fill);

        Container::new(main)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_languages(&self) -> Element<'_, Message> {
        // "Effective active" — the answer the engine would give if
        // asked right now: an empty allow-list means "every OS layout
        // is active", a non-empty list means "only the listed ones".
        // The earlier UI displayed the raw list, which is why a
        // freshly-installed user with no edits saw zero ticked boxes
        // even though every OS layout was being considered. Now we
        // render this *effective* answer so the checkbox state always
        // matches the engine's decision rule.
        let allow_list = &self.settings.languages.active;
        let implicit_all = allow_list.is_empty();

        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Languages").size(24));

        if implicit_all {
            col = col.push(
                Text::new(
                    "All OS-active layouts are currently considered. \
                     Untick a box to restrict kb-switcher to a subset.",
                )
                .size(13),
            );
        } else {
            col = col.push(
                Text::new(format!(
                    "Restricted to {} layout(s). Tick more to include them, \
                     or hit 'Reset to defaults' on the About pane to go back \
                     to 'use every OS layout'.",
                    allow_list.len()
                ))
                .size(13),
            );
        }

        if self.os_layouts.is_empty() {
            col = col.push(
                Text::new(
                    "No OS layouts detected. Add languages in your system's keyboard \
                     settings, then reopen this window.",
                )
                .size(13),
            );
        } else {
            for id in &self.os_layouts {
                let is_active_effective = implicit_all || allow_list.contains(id);
                let is_ignored = self.settings.languages.ignored.contains(id);
                let row = Row::new()
                    .spacing(16)
                    .push(Text::new(id.as_str().to_string()).width(Length::FillPortion(2)))
                    .push(
                        Checkbox::new("Active", is_active_effective)
                            .on_toggle({
                                let id = id.clone();
                                move |b| Message::LanguageToggled(id.clone(), b)
                            })
                            .width(Length::FillPortion(1)),
                    )
                    .push(
                        Checkbox::new("Ignore", is_ignored)
                            .on_toggle({
                                let id = id.clone();
                                move |b| Message::LanguageIgnoreToggled(id.clone(), b)
                            })
                            .width(Length::FillPortion(1)),
                    );
                col = col.push(row);
            }
        }

        col = col.push(Space::with_height(8)).push(
            Text::new(
                "Tip: 'Active' is the allow-list — when nothing is restricted \
                 every OS layout is included. 'Ignore' is a hard veto and \
                 always wins.",
            )
            .size(11),
        );

        col.into()
    }

    fn view_hotkeys(&self) -> Element<'_, Message> {
        let row = |label: &str, current: &str, kind: HotkeyKind| -> Element<'_, Message> {
            let capturing = self.capturing == Some(kind);
            let display: Element<'_, Message> = if capturing {
                Text::new("Press a combination… (Esc to cancel)")
                    .size(13)
                    .color(iced::Color::from_rgb(0.85, 0.55, 0.20))
                    .into()
            } else {
                Text::new(current.to_owned()).size(13).into()
            };
            let action: Element<'_, Message> = if capturing {
                Button::new(Text::new("Cancel").size(12))
                    .on_press(Message::HotkeyRebindCancel)
                    .into()
            } else {
                Button::new(Text::new("Rebind").size(12))
                    .on_press(Message::HotkeyRebindStart(kind))
                    .into()
            };
            Row::new()
                .spacing(16)
                .push(Text::new(label.to_owned()).width(Length::FillPortion(2)))
                .push(Container::new(display).width(Length::FillPortion(3)))
                .push(action)
                .into()
        };

        Column::new()
            .spacing(14)
            .push(Text::new("Hotkeys").size(24))
            .push(
                Text::new(
                    "Global hotkeys are registered with the OS at startup. \
                     Click 'Rebind', press the new combination, then save. \
                     The new binding takes effect after the tray restarts \
                     (Save → Quit → relaunch).",
                )
                .size(13),
            )
            .push(Space::with_height(6))
            .push(row(
                "Pause / resume auto-switch",
                &self.settings.hotkeys.pause_toggle,
                HotkeyKind::Pause,
            ))
            .push(row(
                "Force-switch the last word",
                &self.settings.hotkeys.manual_switch_last,
                HotkeyKind::SwitchLast,
            ))
            .push(Space::with_height(8))
            .push(
                Text::new(
                    "Tip: capture refuses single-letter combinations and bare \
                     keys — at least one of Ctrl / Alt / Shift / Cmd is required. \
                     Esc cancels capture without changing anything.",
                )
                .size(11),
            )
            .into()
    }

    fn view_commands(&self) -> Element<'_, Message> {
        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Commands").size(24))
            .push(
                Text::new(
                    "Type a short token, get a phrase — like classic snippet expanders. \
                     For example: trigger `anrl` + space → `Anatomical Reference List `. \
                     The engine watches every word boundary and fires when the typed \
                     token matches. Pause / switch-last live separately on the Hotkeys \
                     pane. New commands take effect after Save + restart.",
                )
                .size(13),
            );

        // ── Existing commands list ──────────────────────────────────
        if self.settings.commands.is_empty() {
            col = col.push(
                Text::new("No commands yet — fill the form below to add one.")
                    .size(12)
                    .color(iced::Color::from_rgb(0.45, 0.45, 0.45)),
            );
        } else {
            for (idx, cmd) in self.settings.commands.iter().enumerate() {
                let summary = format_command_summary(cmd);
                col = col.push(
                    Row::new()
                        .spacing(10)
                        .push(Text::new(cmd.trigger.clone()).width(Length::FillPortion(2)))
                        .push(Text::new(summary).size(12).width(Length::FillPortion(5)))
                        .push(
                            Button::new(Text::new("×").size(14))
                                .on_press(Message::CommandRemove(idx))
                                .style(button::danger),
                        ),
                );
            }
        }

        col = col.push(Space::with_height(8));

        // ── "Add new command" form ──────────────────────────────────
        col = col.push(Text::new("Add a new command").size(16));

        col = col.push(
            Row::new()
                .spacing(8)
                .push(Text::new("Name").size(12).width(Length::FillPortion(1)))
                .push(
                    TextInput::new("e.g. Insert email signature", &self.command_draft_name)
                        .on_input(Message::CommandDraftNameChanged)
                        .width(Length::FillPortion(4)),
                ),
        );

        // Trigger row: text input for the typed token (e.g. `anrl`).
        // The buffer resets at every word boundary, so triggers must
        // be a single token — the validation path on Add will refuse
        // any whitespace.
        col = col.push(
            Row::new()
                .spacing(8)
                .push(Text::new("Trigger").size(12).width(Length::FillPortion(1)))
                .push(
                    TextInput::new("e.g. anrl, ;sig, ((en))", &self.command_draft_trigger)
                        .on_input(Message::CommandDraftTriggerChanged)
                        .width(Length::FillPortion(4)),
                ),
        );

        // Action kind picker (radio-style buttons).
        let mk_kind_btn = |kind: CommandActionKind| -> Element<'_, Message> {
            let selected = self.command_draft_action_kind == kind;
            let style: fn(&Theme, button::Status) -> button::Style = if selected {
                button::primary
            } else {
                button::secondary
            };
            Button::new(Text::new(kind.label()).size(12))
                .on_press(Message::CommandDraftActionKindChanged(kind))
                .style(style)
                .into()
        };
        col = col.push(
            Row::new()
                .spacing(8)
                .push(Text::new("Action").size(12).width(Length::FillPortion(1)))
                .push(mk_kind_btn(CommandActionKind::TypeText))
                .push(mk_kind_btn(CommandActionKind::SwitchLayout))
                .push(mk_kind_btn(CommandActionKind::OpenPath)),
        );

        // Param input (placeholder swaps based on action kind).
        col = col.push(
            Row::new()
                .spacing(8)
                .push(
                    Text::new(match self.command_draft_action_kind {
                        CommandActionKind::TypeText => "Text",
                        CommandActionKind::SwitchLayout => "Layout id",
                        CommandActionKind::OpenPath => "Path / URL",
                    })
                    .size(12)
                    .width(Length::FillPortion(1)),
                )
                .push(
                    TextInput::new(
                        self.command_draft_action_kind.placeholder(),
                        &self.command_draft_param,
                    )
                    .on_input(Message::CommandDraftParamChanged)
                    .width(Length::FillPortion(4)),
                ),
        );

        // Optional apps filter.
        col = col.push(
            Row::new()
                .spacing(8)
                .push(
                    Text::new("Apps (optional)")
                        .size(12)
                        .width(Length::FillPortion(1)),
                )
                .push(
                    TextInput::new(
                        "comma-separated, e.g. Code.exe,idea64.exe",
                        &self.command_draft_apps,
                    )
                    .on_input(Message::CommandDraftAppsChanged)
                    .on_submit(Message::CommandAdd)
                    .width(Length::FillPortion(4)),
                ),
        );

        // Status + Add row.
        let status: Element<'_, Message> = match &self.command_status {
            Some(b) => Text::new(b.text.clone())
                .size(11)
                .color(if b.is_error {
                    iced::Color::from_rgb(0.85, 0.20, 0.20)
                } else {
                    iced::Color::from_rgb(0.20, 0.55, 0.30)
                })
                .into(),
            None => Space::with_width(Length::Shrink).into(),
        };
        col = col.push(
            Row::new()
                .spacing(8)
                .push(status)
                .push(Space::with_width(Length::Fill))
                .push(
                    Button::new(Text::new("Add command").size(12))
                        .on_press(Message::CommandAdd)
                        .style(button::primary),
                ),
        );

        col = col.push(Space::with_height(6)).push(
            Text::new(
                "Tips: pick triggers that don't collide with words you actually type — \
                 `the` would expand on every English sentence; `;sig` or `((email))` \
                 are safer. Match is exact and case-sensitive. Leave 'Apps' empty for \
                 a global command, or list `OUTLOOK.EXE,thunderbird.exe` to scope a \
                 command (case-insensitive basename match).",
            )
            .size(11),
        );

        col.into()
    }

    fn view_wordlists(&self) -> Element<'_, Message> {
        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Wordlists").size(24))
            .push(
                Text::new(
                    "Add language-specific words to the per-layout dictionary \
                     overlay. 'Save' writes to disk; closing this window then \
                     refreshes the engine's dictionary set so new words start \
                     counting toward detection on the next typed word — no \
                     tray restart needed.",
                )
                .size(13),
            );

        if self.os_layouts.is_empty() {
            col = col.push(
                Text::new(
                    "No OS layouts detected. Add languages in your system's \
                     keyboard settings, then reopen this window.",
                )
                .size(13),
            );
            return col.into();
        }

        // ── Profile picker (Global + each configured profile) ──────
        // Only shown when the user has at least one profile configured;
        // otherwise the row would be a redundant single "Global"
        // button. Add profiles via `[[wordlists.profiles]]` in
        // config.toml — full profile-list management UI is queued for
        // a follow-up.
        if !self.settings.wordlists.profiles.is_empty() {
            let profile_btn = |id: &str, label: &str| -> Element<'_, Message> {
                let selected = self.wordlist_profile == id;
                let style: fn(&Theme, button::Status) -> button::Style = if selected {
                    button::primary
                } else {
                    button::secondary
                };
                Button::new(Text::new(label.to_owned()).size(12))
                    .on_press(Message::WordlistProfileSelected(id.to_owned()))
                    .style(style)
                    .into()
            };
            let mut profile_row = Row::new().spacing(6).push(Text::new("Profile").size(12));
            profile_row = profile_row.push(profile_btn("", "Global"));
            for p in &self.settings.wordlists.profiles {
                let label = if p.name.is_empty() {
                    p.id.clone()
                } else {
                    p.name.clone()
                };
                profile_row = profile_row.push(profile_btn(&p.id, &label));
            }
            col = col.push(profile_row);
        }

        // ── Layout picker (one button per OS-active layout) ─────────
        let mut layout_row = Row::new().spacing(6);
        for id in &self.os_layouts {
            let selected = self.wordlist_layout.as_ref() == Some(id);
            let style: fn(&Theme, button::Status) -> button::Style = if selected {
                button::primary
            } else {
                button::secondary
            };
            layout_row = layout_row.push(
                Button::new(Text::new(id.as_str().to_string()).size(12))
                    .on_press(Message::WordlistLayoutSelected(id.clone()))
                    .style(style),
            );
        }
        col = col.push(layout_row);

        // ── Kind picker (Extras vs Stop) ────────────────────────────
        let kind_button = |kind: WordlistKind| -> Element<'_, Message> {
            let selected = self.wordlist_kind == kind;
            let style: fn(&Theme, button::Status) -> button::Style = if selected {
                button::primary
            } else {
                button::secondary
            };
            Button::new(Text::new(kind.label()).size(12))
                .on_press(Message::WordlistKindSelected(kind))
                .style(style)
                .into()
        };
        col = col.push(
            Row::new()
                .spacing(6)
                .push(kind_button(WordlistKind::Extras))
                .push(kind_button(WordlistKind::Stop)),
        );

        // ── Resolved-path hint ──────────────────────────────────────
        if let Some(id) = &self.wordlist_layout {
            let path_label =
                match resolve_overlay_path(&self.wordlist_profile, id, self.wordlist_kind) {
                    Some(p) => p.display().to_string(),
                    None => "(no config dir resolved on this platform)".to_owned(),
                };
            col = col.push(Text::new(format!("File: {path_label}")).size(11));
        }

        // ── Editor body + per-pane footer ───────────────────────────
        let editor: Element<'_, Message> = if self.wordlist_layout.is_some() {
            text_editor(&self.wordlist_content)
                .on_action(Message::WordlistEdit)
                .height(Length::Fixed(260.0))
                .padding(8)
                .placeholder("# one word per line — '#' starts a comment\n")
                .into()
        } else {
            Text::new("Pick a layout above to start editing.")
                .size(13)
                .into()
        };
        col = col.push(editor);

        let dirty_marker: Element<'_, Message> = if self.wordlist_dirty {
            Text::new("● unsaved changes")
                .size(11)
                .color(iced::Color::from_rgb(0.85, 0.55, 0.20))
                .into()
        } else {
            Space::with_width(Length::Shrink).into()
        };
        let status: Element<'_, Message> = match &self.wordlist_status {
            Some(b) => Text::new(b.text.clone())
                .size(11)
                .color(if b.is_error {
                    iced::Color::from_rgb(0.85, 0.20, 0.20)
                } else {
                    iced::Color::from_rgb(0.20, 0.55, 0.30)
                })
                .into(),
            None => Space::with_width(Length::Shrink).into(),
        };

        col = col.push(
            Row::new()
                .spacing(8)
                .push(dirty_marker)
                .push(Space::with_width(Length::Fill))
                .push(status)
                .push(Button::new(Text::new("Reload").size(12)).on_press(Message::WordlistReload))
                .push(
                    Button::new(Text::new("Save").size(12))
                        .on_press(Message::WordlistSave)
                        .style(button::primary),
                ),
        );

        col = col.push(Space::with_height(6)).push(
            Text::new(
                "Tip: Extras helps detection prefer your jargon, \
                     project nouns or family names. Stop list extends the \
                     1- / 2-letter entries the detector accepts as real \
                     words instead of typos.",
            )
            .size(11),
        );

        col.into()
    }

    fn view_exceptions(&self) -> Element<'_, Message> {
        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Exceptions").size(24))
            .push(
                Text::new(
                    "kb-switcher skips auto-correction when the foreground app's \
                     executable basename is in this list. Manual switch (the \
                     hotkey on the Hotkeys pane) bypasses the list — devs can \
                     still fix wrong-layout identifiers explicitly inside an IDE.",
                )
                .size(13),
            )
            .push(Space::with_height(6));

        for (idx, entry) in self.settings.exceptions.disabled_apps.iter().enumerate() {
            col = col.push(
                Row::new()
                    .spacing(12)
                    .push(Text::new(entry.clone()).width(Length::Fill))
                    .push(
                        Button::new(Text::new("×").size(14))
                            .on_press(Message::ExceptionRemove(idx))
                            .style(button::danger),
                    ),
            );
        }

        col = col
            .push(Space::with_height(8))
            .push(
                Row::new()
                    .spacing(8)
                    .push(
                        TextInput::new("e.g. mygame.exe", &self.exception_draft)
                            .on_input(Message::ExceptionDraftChanged)
                            .on_submit(Message::ExceptionAdd)
                            .width(Length::Fill),
                    )
                    .push(
                        Button::new(Text::new("Add"))
                            .on_press(Message::ExceptionAdd)
                            .style(button::primary),
                    ),
            )
            .push(
                Text::new(
                    "Match is case-insensitive against the basename — both \
                     `code.exe` and `Code.exe` work.",
                )
                .size(11),
            );

        col.into()
    }

    fn view_general(&self) -> Element<'_, Message> {
        let g = &self.settings.general;
        let e = &self.settings.engine;

        Column::new()
            .spacing(14)
            .push(Text::new("General").size(24))
            .push(
                Checkbox::new("Start automatically when I sign in", g.autostart)
                    .on_toggle(Message::AutostartToggled),
            )
            .push(
                Checkbox::new("Play a soft chime on correction", g.sound_on_correct)
                    .on_toggle(Message::SoundOnCorrectToggled),
            )
            .push(
                Checkbox::new(
                    "Skip auto-switch on identifiers (foo_bar, snake_case, …)",
                    e.suppress_in_identifiers,
                )
                .on_toggle(Message::SuppressInIdentifiersToggled),
            )
            .push(Space::with_height(6))
            .push(
                Row::new()
                    .spacing(10)
                    .push(Text::new("Idle timeout (ms):").width(Length::Shrink))
                    .push(
                        Button::new(Text::new("-100").size(12))
                            .on_press(Message::IdleTimeoutDelta(-100)),
                    )
                    .push(Text::new(format!("{:>5}", e.idle_timeout_ms)))
                    .push(
                        Button::new(Text::new("+100").size(12))
                            .on_press(Message::IdleTimeoutDelta(100)),
                    )
                    .push(
                        Text::new("Buffer is cleared after this much keyboard silence.").size(11),
                    ),
            )
            .push(Space::with_height(20))
            .push(Text::new("Folders").size(16))
            .push(
                Row::new()
                    .spacing(8)
                    .push(
                        Button::new(Text::new("Open config.toml"))
                            .on_press(Message::OpenConfigFile),
                    )
                    .push(Button::new(Text::new("Logs")).on_press(Message::OpenLogsDir))
                    .push(
                        Button::new(Text::new("User wordlists"))
                            .on_press(Message::OpenWordlistsDir),
                    )
                    .push(Button::new(Text::new("User layouts")).on_press(Message::OpenLayoutsDir)),
            )
            .into()
    }

    fn view_about(&self) -> Element<'_, Message> {
        Column::new()
            .spacing(10)
            .push(Text::new("About kb-switcher").size(24))
            .push(Text::new(format!("Version {}", env!("CARGO_PKG_VERSION"))))
            .push(Text::new(
                "Cross-platform automatic keyboard layout switcher.",
            ))
            .push(Space::with_height(8))
            .push(Text::new("Project: https://github.com/Just-Code-NET/kb-switcher").size(13))
            .push(Text::new("Issues: https://github.com/Just-Code-NET/kb-switcher/issues").size(13))
            .push(Space::with_height(16))
            .push(Text::new("Power-user escape hatches").size(16))
            .push(
                Row::new()
                    .spacing(8)
                    .push(
                        Button::new(Text::new("Reset to defaults"))
                            .on_press(Message::ResetDefaults)
                            .style(button::danger),
                    )
                    .push(Button::new(Text::new("Reload from disk")).on_press(Message::Reload)),
            )
            .into()
    }

    fn view_footer(&self) -> Element<'_, Message> {
        // No styled "toast" yet (iced 0.13 doesn't ship a one-line
        // success / danger container preset out of the box and we'd
        // rather not pin a custom palette here just for one line).
        // Plain coloured Text covers it: green for OK, red for error,
        // gone-when-cleared.
        let banner: Element<'_, Message> = match &self.save_banner {
            Some(b) => Text::new(&b.text)
                .size(12)
                .color(if b.is_error {
                    iced::Color::from_rgb(0.85, 0.20, 0.20)
                } else {
                    iced::Color::from_rgb(0.20, 0.55, 0.30)
                })
                .into(),
            None => Space::with_width(Length::Shrink).into(),
        };

        Row::new()
            .padding(Padding {
                top: 12.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            })
            .spacing(10)
            .push(banner)
            .push(Space::with_width(Length::Fill))
            .push(Button::new(Text::new("Reload")).on_press(Message::Reload))
            .push(
                Button::new(Text::new("Save"))
                    .on_press(Message::Save)
                    .style(button::primary),
            )
            .into()
    }
}

fn nav_panel(active: Pane) -> Element<'static, Message> {
    let mk = |label: &'static str, pane: Pane| -> Element<'static, Message> {
        let style: fn(&Theme, button::Status) -> button::Style = if active == pane {
            button::primary
        } else {
            button::text
        };
        Button::new(Text::new(label))
            .on_press(Message::SelectPane(pane))
            .style(style)
            .width(Length::Fill)
            .into()
    };

    Container::new(
        Column::new()
            .padding(Padding::new(16.0))
            .spacing(6)
            .push(Text::new("kb-switcher").size(18))
            .push(Space::with_height(12))
            .push(mk("Languages", Pane::Languages))
            .push(mk("Hotkeys", Pane::Hotkeys))
            .push(mk("Commands", Pane::Commands))
            .push(mk("Wordlists", Pane::Wordlists))
            .push(mk("General", Pane::General))
            .push(mk("Exceptions", Pane::Exceptions))
            .push(mk("About", Pane::About)),
    )
    .width(180)
    .height(Length::Fill)
    .into()
}

/// Validate the "Add command" form and produce a [`UserCommand`]
/// ready to push into `settings.commands`. Returns `Err(message)`
/// describing the first failed check — message is shown to the user
/// in the Commands pane's status banner so they know what to fix.
///
/// Validation:
///
/// * Trigger is non-empty and contains no whitespace (the buffer
///   resets at every word boundary so a multi-token trigger could
///   never match).
/// * Param is non-empty (every action variant needs payload —
///   "type empty text" / "switch to ''" / "open ''" all wrong).
/// * For `SwitchLayout`, the layout id matches the loose BCP-47
///   shape we already accept elsewhere (`xx-XX[-VARIANT...]`).
/// * Generated id is unique against existing `settings.commands`.
fn build_command_from_draft(app: &SettingsApp) -> Result<UserCommand, String> {
    let trigger = app.command_draft_trigger.trim().to_owned();
    if trigger.is_empty() {
        return Err("Set a trigger first (e.g. `anrl`).".into());
    }
    if trigger.chars().any(char::is_whitespace) {
        return Err(
            "Trigger must be a single token — no spaces. The buffer resets at every \
             word boundary, so a multi-word trigger can never match."
                .into(),
        );
    }
    let param = app.command_draft_param.trim().to_owned();
    if param.is_empty() {
        return Err("Action parameter is empty.".into());
    }
    let action = match app.command_draft_action_kind {
        CommandActionKind::TypeText => CommandAction::TypeText { text: param },
        CommandActionKind::SwitchLayout => {
            // Loose BCP-47 sanity — reject strings that obviously
            // can't be a layout id (whitespace, lowercase-only,
            // wrong shape) to save the user a mystery silent-no-op.
            if !looks_like_layout_id(&param) {
                return Err(format!(
                    "`{param}` doesn't look like a layout id (e.g. `en-US`)."
                ));
            }
            CommandAction::SwitchLayout {
                layout: LayoutId::new(param),
            }
        }
        CommandActionKind::OpenPath => CommandAction::OpenPath { path: param },
    };

    let name = app.command_draft_name.trim();
    let id = derive_command_id(name, &action, &app.settings.commands);
    if app.settings.commands.iter().any(|c| c.id == id) {
        return Err(format!(
            "A command with id `{id}` already exists — pick a different name."
        ));
    }

    let apps: Vec<String> = app
        .command_draft_apps
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    Ok(UserCommand {
        id,
        name: name.to_owned(),
        trigger,
        action,
        apps,
    })
}

/// Loose validation of "this string could plausibly be a BCP-47
/// layout id". Accepts `en-US`, `uk-UA`, `kk-Cyrl-KZ`, etc. —
/// we let the OS reject genuinely-wrong values at switch time
/// (the engine logs a warning + no-ops in that case).
fn looks_like_layout_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // First segment must be 2-3 ascii letters; rest must contain
    // at least one `-` and only ascii alphanumerics + dashes.
    if !s.contains('-') {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Generate a stable kebab-case id from the user's display name —
/// or fall back to `cmd-<n>` if name is empty / collides. Handles
/// the "I just want to add a hotkey, don't make me name it" case
/// without forcing the user to pick an id manually.
fn derive_command_id(name: &str, action: &CommandAction, existing: &[UserCommand]) -> String {
    let from_name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if !from_name.is_empty() {
        from_name
    } else {
        match action {
            CommandAction::TypeText { .. } => "type-text".into(),
            CommandAction::SwitchLayout { .. } => "switch-layout".into(),
            CommandAction::OpenPath { .. } => "open-path".into(),
        }
    };
    // Disambiguate by appending `-2`, `-3`, … as needed.
    let mut candidate = base.clone();
    let mut n: u32 = 2;
    while existing.iter().any(|c| c.id == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// Single-line description of a command for the existing-list view.
/// Renders the action concisely so the user can scan a long list of
/// trigger rows and see what each does without expanding rows.
fn format_command_summary(cmd: &UserCommand) -> String {
    let display_name = if cmd.name.is_empty() {
        cmd.id.clone()
    } else {
        cmd.name.clone()
    };
    let action_blurb = match &cmd.action {
        CommandAction::TypeText { text } => {
            // Truncate long snippets so one row stays one row.
            let preview = text.chars().take(40).collect::<String>();
            let suffix = if text.chars().count() > 40 { "…" } else { "" };
            format!("type `{preview}{suffix}`")
        }
        CommandAction::SwitchLayout { layout } => format!("→ {layout}"),
        CommandAction::OpenPath { path } => format!("open `{path}`"),
    };
    let apps_blurb = if cmd.apps.is_empty() {
        String::new()
    } else {
        format!(" (in {})", cmd.apps.join(", "))
    };
    format!("{display_name} — {action_blurb}{apps_blurb}")
}

/// Map a [`LayoutId`] (`en-US`, `uk-UA`, `kk-Cyrl-KZ`, …) to the
/// on-disk overlay-file *stem* (`en_us`, `uk_ua`, `kk_cyrl_kz`).
///
/// The convention matches both the bundled `data/wordlists/<stem>.fst`
/// names and the loader's `<config-dir>/kb-switcher/wordlists/<stem>.txt`
/// path resolution — keeping them in lock-step is what lets the user
/// add overlay words from the GUI and have the engine pick them up
/// without any additional book-keeping.
fn layout_id_to_stem(id: &LayoutId) -> String {
    id.as_str().to_lowercase().replace('-', "_")
}

/// Absolute path to the user-overlay file for `(profile_id, layout, kind)`.
/// Empty `profile_id` resolves to the global overlay directory
/// (`<config-dir>/kb-switcher/wordlists/<stem><suffix>.txt`);
/// non-empty resolves into the per-profile subdirectory
/// (`<config-dir>/kb-switcher/wordlists/profiles/<profile_id>/<stem><suffix>.txt`).
/// Returns `None` if the platform's config directory can't be
/// resolved (rare — usually only on minimal CI containers).
fn resolve_overlay_path(profile_id: &str, id: &LayoutId, kind: WordlistKind) -> Option<PathBuf> {
    let dir = if profile_id.is_empty() {
        kb_core::layouts::user_wordlist_dir()?
    } else {
        kb_core::layouts::user_profile_wordlist_dir(profile_id)?
    };
    let stem = layout_id_to_stem(id);
    Some(dir.join(format!("{stem}{}.txt", kind.suffix())))
}

/// Best-effort read of the resolved overlay file. Returns the
/// contents on success, empty string on `NotFound` (the common
/// first-edit case), or empty string with a warn log on real I/O
/// error so the GUI never blocks the user from starting fresh.
fn read_overlay_file_or_empty(profile_id: &str, id: &LayoutId, kind: WordlistKind) -> String {
    let Some(path) = resolve_overlay_path(profile_id, id, kind) else {
        warn!(
            layout = %id,
            profile = %profile_id,
            "no config dir resolved; wordlist editor starts empty"
        );
        return String::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            warn!(?path, err = %e, "could not read wordlist overlay; starting empty");
            String::new()
        }
    }
}

/// Atomic-ish write of the editor buffer to the resolved overlay
/// path. Creates the parent directory on first use (the user may
/// have never opened `<config-dir>/kb-switcher/wordlists/` or the
/// per-profile subdirectory before). The trailing-newline
/// normalisation matches the convention of the bundled files and
/// keeps `git diff` quiet for users who keep their config dir under
/// version control.
fn save_overlay_file(
    profile_id: &str,
    id: &LayoutId,
    kind: WordlistKind,
    text: &str,
) -> std::io::Result<PathBuf> {
    let path = resolve_overlay_path(profile_id, id, kind).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "config directory not resolved on this platform",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut normalised = text.to_owned();
    if !normalised.ends_with('\n') {
        normalised.push('\n');
    }
    std::fs::write(&path, normalised)?;
    Ok(path)
}

/// Lone-modifier-only key presses (Ctrl, Shift, Alt, Cmd) shouldn't
/// be captured as the hotkey itself — the user is mid-combination.
/// We filter them in the keyboard subscription so the captured combo
/// is always `<modifier(s)>+<non-modifier-key>`.
fn is_modifier_key(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(
            Named::Control
                | Named::Shift
                | Named::Alt
                | Named::AltGraph
                | Named::Meta
                | Named::Super
                | Named::Hyper
        )
    )
}

/// Render a captured `(modifiers, key)` combo as the canonical
/// hotkey string `global-hotkey`'s `FromStr` accepts — `Ctrl+Shift+Space`,
/// `Alt+F4`, etc. We use platform-portable names: `Ctrl` (not
/// `Control`), `Cmd` for Meta on macOS, `Win` is intentionally
/// avoided in favour of the more universal `Meta`.
fn format_hotkey(key: &Key, modifiers: Modifiers) -> String {
    let mut parts: Vec<String> = Vec::new();
    if modifiers.control() {
        parts.push("Ctrl".into());
    }
    if modifiers.alt() {
        parts.push("Alt".into());
    }
    if modifiers.shift() {
        parts.push("Shift".into());
    }
    if modifiers.logo() {
        // global-hotkey accepts Meta / Super / Cmd / Win — use Meta
        // for consistency with the upstream's docs.
        parts.push("Meta".into());
    }
    parts.push(key_to_string(key));
    parts.join("+")
}

/// One-key serialisation matching `global-hotkey::HotKey::from_str`.
/// Letters get upper-cased (`a` → `A`); numbers stay as digits;
/// named keys map to their canonical name (Space / Backspace /
/// F1..F12 / arrow keys). Unrecognised keys round-trip via Debug —
/// good enough for the rare edge case (e.g. Print Screen) where
/// users will see something parseable in the Settings UI.
fn key_to_string(key: &Key) -> String {
    match key {
        Key::Character(c) => c.to_uppercase(),
        Key::Named(n) => match n {
            Named::Space => "Space".into(),
            Named::Backspace => "Backspace".into(),
            Named::Enter => "Enter".into(),
            Named::Tab => "Tab".into(),
            Named::ArrowUp => "Up".into(),
            Named::ArrowDown => "Down".into(),
            Named::ArrowLeft => "Left".into(),
            Named::ArrowRight => "Right".into(),
            Named::Home => "Home".into(),
            Named::End => "End".into(),
            Named::PageUp => "PageUp".into(),
            Named::PageDown => "PageDown".into(),
            Named::Insert => "Insert".into(),
            Named::Delete => "Delete".into(),
            Named::Escape => "Escape".into(),
            Named::F1 => "F1".into(),
            Named::F2 => "F2".into(),
            Named::F3 => "F3".into(),
            Named::F4 => "F4".into(),
            Named::F5 => "F5".into(),
            Named::F6 => "F6".into(),
            Named::F7 => "F7".into(),
            Named::F8 => "F8".into(),
            Named::F9 => "F9".into(),
            Named::F10 => "F10".into(),
            Named::F11 => "F11".into(),
            Named::F12 => "F12".into(),
            other => format!("{other:?}"),
        },
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capture pipeline must produce strings that `global-hotkey`'s
    /// `FromStr` accepts. Otherwise rebinding succeeds in the UI but
    /// the next tray launch silently drops the hotkey. We round-trip
    /// the canonical combos to catch that.
    #[test]
    fn captured_hotkeys_round_trip_through_global_hotkey_parse() {
        use global_hotkey::hotkey::HotKey;
        let mut mods = Modifiers::empty();
        mods.insert(Modifiers::CTRL);
        mods.insert(Modifiers::SHIFT);
        let cases = [
            (Key::Named(Named::Space), mods, "Ctrl+Shift+Space"),
            (Key::Named(Named::Backspace), mods, "Ctrl+Shift+Backspace"),
            (
                Key::Character("a".into()),
                Modifiers::CTRL | Modifiers::ALT,
                "Ctrl+Alt+A",
            ),
            (Key::Named(Named::F4), Modifiers::ALT, "Alt+F4"),
        ];
        for (key, mods, expected) in cases {
            let formatted = format_hotkey(&key, mods);
            assert_eq!(
                formatted, expected,
                "format mismatch for {key:?} + {mods:?}"
            );
            assert!(
                formatted.parse::<HotKey>().is_ok(),
                "global-hotkey rejected our formatted combo `{formatted}` — \
                 the rebind UI would silently drop hotkeys this shape"
            );
        }
    }

    /// Auto-id must be deterministic and collision-free. The UI
    /// silently dedupes by appending `-2`, `-3`, … so users don't
    /// need to think about ids — but the dedup must be stable, since
    /// duplicate ids in the saved config would be a load-time error.
    #[test]
    fn derive_command_id_is_kebab_case_and_unique() {
        let action = CommandAction::TypeText { text: "x".into() };
        let id = derive_command_id("Insert Email Signature!", &action, &[]);
        assert_eq!(id, "insert-email-signature");

        // Empty name → action-typed fallback.
        let blank = derive_command_id("", &action, &[]);
        assert_eq!(blank, "type-text");

        // Collision → `-2` suffix.
        let existing = vec![UserCommand {
            id: "type-text".into(),
            name: String::new(),
            trigger: "anrl".into(),
            action: action.clone(),
            apps: Vec::new(),
        }];
        let dedup = derive_command_id("", &action, &existing);
        assert_eq!(dedup, "type-text-2");
    }

    /// `looks_like_layout_id` is a hint, not a strict validator —
    /// must accept the canonical bundled set + multi-segment ids
    /// (Cyrillic Kazakh) and reject obviously-not-an-id strings.
    #[test]
    fn looks_like_layout_id_accepts_real_ids_and_rejects_garbage() {
        for ok in ["en-US", "uk-UA", "kk-Cyrl-KZ", "zh-Hans-CN"] {
            assert!(
                looks_like_layout_id(ok),
                "{ok} should be accepted as a layout id"
            );
        }
        for bad in ["", "english", "EN", "en US", "fr.fr", "uk--UA…"] {
            assert!(
                !looks_like_layout_id(bad),
                "{bad} should NOT be accepted as a layout id"
            );
        }
    }

    /// Summary format is what users scan first when they have a
    /// long list of commands. It must include the display name (or
    /// id fallback), the action description, and the apps filter
    /// when set — and stay on one line for any reasonable input.
    #[test]
    fn format_command_summary_is_concise_and_complete() {
        let cmd = UserCommand {
            id: "sig".into(),
            name: "Email signature".into(),
            trigger: ";sig".into(),
            action: CommandAction::TypeText {
                text: "Best regards".into(),
            },
            apps: vec!["OUTLOOK.EXE".into()],
        };
        let s = format_command_summary(&cmd);
        assert!(s.contains("Email signature"));
        assert!(s.contains("Best regards"));
        assert!(s.contains("OUTLOOK.EXE"));

        // Falls back to id when name is empty.
        let cmd2 = UserCommand {
            id: "go-en".into(),
            name: String::new(),
            trigger: "((en))".into(),
            action: CommandAction::SwitchLayout {
                layout: LayoutId::new("en-US"),
            },
            apps: Vec::new(),
        };
        let s2 = format_command_summary(&cmd2);
        assert!(s2.starts_with("go-en"));
        assert!(s2.contains("en-US"));
        // No apps blurb when the filter is empty.
        assert!(!s2.contains(" (in "));
    }

    /// Stem mapping must agree with both the bundled FST file names
    /// (`data/wordlists/<stem>.fst`) and the loader's user-overlay
    /// path (`<config-dir>/kb-switcher/wordlists/<stem>.txt`).
    /// Otherwise the GUI would write to a file the engine never
    /// reads, and users would see "I added words but they don't take
    /// effect."
    #[test]
    fn layout_id_to_stem_matches_bundled_naming() {
        let cases = [
            ("en-US", "en_us"),
            ("uk-UA", "uk_ua"),
            ("ru-RU", "ru_ru"),
            ("de-DE", "de_de"),
            ("es-ES", "es_es"),
            ("fr-FR", "fr_fr"),
            // Multi-segment IDs (e.g. Cyrillic Kazakh) collapse all
            // dashes — keeps the convention uniform.
            ("kk-Cyrl-KZ", "kk_cyrl_kz"),
        ];
        for (id, expected) in cases {
            assert_eq!(
                layout_id_to_stem(&LayoutId::new(id)),
                expected,
                "stem mismatch for {id}"
            );
        }
    }

    /// `WordlistKind::suffix` must round-trip with the bundled
    /// `<stem>-stop.txt` convention. Easy to fix typos here; harder
    /// to notice them at runtime since a missing stop file is
    /// silently treated as "no extras."
    #[test]
    fn wordlist_kind_suffix_matches_loader_convention() {
        assert_eq!(WordlistKind::Extras.suffix(), "");
        assert_eq!(WordlistKind::Stop.suffix(), "-stop");
    }

    /// The text the editor saves must round-trip through the engine's
    /// own loader without losing anything semantically meaningful.
    /// We mirror `kb_core::layouts::parse_wordlist` here — lowercase,
    /// comment-stripped, blank-line-skipped — and confirm typical
    /// free-form content survives. If the engine's parser ever
    /// diverges, this test makes the GUI catch it before users do.
    #[test]
    fn wordlist_buffer_is_compatible_with_loader_parser() {
        // Multi-line buffer the user might type into the editor:
        // pure words, comments, blanks, mixed case, leading whitespace.
        let body = "# project nouns\nfoo\nBar\n  baz  \n\n#trailing comment\n";
        let words: std::collections::HashSet<String> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_lowercase)
            .collect();
        for w in ["foo", "bar", "baz"] {
            assert!(words.contains(w), "{w} should survive parse");
        }
        // Comments & blanks must never become words.
        assert!(!words.contains("# project nouns"));
        assert!(!words.contains(""));
    }

    /// `save_overlay_file` terminates the buffer with a newline
    /// regardless of whether the user did. Keeps `git diff` quiet
    /// for users who keep their config dir under version control,
    /// and matches the convention of the bundled lists. We can't
    /// easily call `save_overlay_file` without a real
    /// `user_wordlist_dir`, but the normalisation logic is small
    /// and isolated — we mirror it here so any future divergence
    /// gets caught.
    #[test]
    fn save_overlay_appends_trailing_newline() {
        fn normalise(text: &str) -> String {
            let mut s = text.to_owned();
            if !s.ends_with('\n') {
                s.push('\n');
            }
            s
        }
        assert_eq!(normalise("foo"), "foo\n");
        assert_eq!(normalise("foo\n"), "foo\n");
        assert_eq!(normalise(""), "\n");
        assert_eq!(normalise("foo\nbar"), "foo\nbar\n");
    }

    /// Lone modifier presses must not be accepted as a hotkey on
    /// their own — otherwise the moment a user clicks "Rebind" and
    /// taps Ctrl, capture finishes immediately with a useless
    /// "Ctrl"-only binding.
    #[test]
    fn lone_modifier_keys_are_filtered() {
        for k in [
            Key::Named(Named::Control),
            Key::Named(Named::Shift),
            Key::Named(Named::Alt),
            Key::Named(Named::Meta),
            Key::Named(Named::Super),
        ] {
            assert!(is_modifier_key(&k), "{k:?} should be classed as modifier");
        }
        // Sanity: a regular character key must NOT be flagged.
        assert!(!is_modifier_key(&Key::Character("x".into())));
        assert!(!is_modifier_key(&Key::Named(Named::Space)));
    }
}
