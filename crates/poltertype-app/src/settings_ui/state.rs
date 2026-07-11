//! `SettingsApp` — the window's whole mutable state.

use std::path::PathBuf;
use std::sync::Arc;

use iced::keyboard::{Key, key::Named};
use iced::widget::text_editor;
use iced::{Subscription, Theme};
use poltertype_core::settings::{Settings, SettingsStore};
use poltertype_layout::LayoutId;

use super::enums::*;
use super::helpers::*;

pub struct SettingsApp {
    pub(super) settings: Settings,
    pub(super) os_layouts: Vec<LayoutId>,
    pub(super) config_path: PathBuf,
    pub(super) store: Arc<SettingsStore>,
    pub(super) pane: Pane,
    /// Set when [`Message::Save`] writes successfully — surfaced as a
    /// transient banner in the footer so the user gets feedback the
    /// click did something.
    pub(super) save_banner: Option<SaveBanner>,
    /// `Some(kind)` while the user is in "press a combination…" mode.
    /// The keyboard subscription consults this to know whether to
    /// route key events to `HotkeyCaptured` or ignore them.
    pub(super) capturing: Option<HotkeyKind>,
    /// Free-form text in the "add a new disabled app" input on the
    /// Exceptions pane. Empty by default; cleared on Add.
    pub(super) exception_draft: String,

    // ── Commands pane: draft of a new command ──────────────────────
    /// Free-form display name. Falls back to id if blank at Add time.
    pub(super) command_draft_name: String,
    /// Trigger token the user types to fire this command. Stored
    /// verbatim — validation happens on the Add path so users
    /// can fix in-place (a `TextInput` with a forced trim would
    /// fight common typing patterns). See [`UserCommand::trigger`].
    pub(super) command_draft_trigger: String,
    /// Which action variant the user picked. Maps to
    /// [`CommandAction`] at Add time using `command_draft_param`.
    pub(super) command_draft_action_kind: CommandActionKind,
    /// Free-form param string. Interpretation depends on
    /// `command_draft_action_kind`:
    ///
    /// * `TypeText`     → literal text snippet (`\n` escapes preserved)
    /// * `SwitchLayout` → BCP-47 id (e.g. `en-US`)
    /// * `OpenPath`     → file path or URL (passed to `opener::open`)
    pub(super) command_draft_param: String,
    /// Optional comma-separated app filter. Empty = all apps.
    pub(super) command_draft_apps: String,
    /// Per-pane status banner (independent of the global save banner
    /// so "Added!" doesn't get clobbered by save state).
    pub(super) command_status: Option<SaveBanner>,

    // ── Wordlists pane ─────────────────────────────────────────────
    /// Currently-selected profile id for editing. Empty string =
    /// the global overlay (`<config-dir>/wordlists/<stem>.txt`);
    /// any non-empty value picks the per-profile directory at
    /// `<config-dir>/wordlists/profiles/<id>/<stem>.txt`. Defaults
    /// to global when the pane opens — same baseline the engine
    /// uses before any focus-driven profile swap happens.
    pub(super) wordlist_profile: String,
    /// Currently-selected layout for editing. `None` until the user
    /// clicks one of the layout buttons (or defaults to the first
    /// OS-active layout when the pane is first opened).
    pub(super) wordlist_layout: Option<LayoutId>,
    /// Which file we're editing for the selected layout.
    pub(super) wordlist_kind: WordlistKind,
    /// Live editor buffer. `text_editor::Content` owns its own state
    /// (cursor position, selection, undo stack) — we just feed
    /// actions in via `Message::WordlistEdit`.
    pub(super) wordlist_content: text_editor::Content,
    /// `Some` once a save / reload / load happens — surfaces a
    /// per-pane status line independent of the global save banner so
    /// "Saved!" on Wordlists doesn't mask "Saved!" on settings.
    pub(super) wordlist_status: Option<SaveBanner>,
    /// Has the buffer been edited since the last load/save? Used to
    /// gate the "discard changes" warning when the user picks a
    /// different layout / kind without saving.
    pub(super) wordlist_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct SaveBanner {
    pub(super) text: String,
    pub(super) is_error: bool,
}

impl SettingsApp {
    pub(super) fn new(
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

    pub(super) fn title(&self) -> String {
        format!("Poltertype · Settings ({})", self.config_path.display())
    }

    pub(super) fn theme(&self) -> Theme {
        // Auto-detect light / dark — feels native on every platform
        // without needing a separate UI toggle.
        Theme::default()
    }

    /// Active subscription. Always listens for window-close requests
    /// (so we can auto-save unsaved wordlist edits before the window
    /// goes away), and *additionally* listens for keyboard events
    /// while we're in hotkey-capture mode. Outside capture mode the
    /// keyboard sub is dropped — otherwise every keystroke in the
    /// window would allocate a `Message` and re-render.
    pub(super) fn subscription(&self) -> Subscription<Message> {
        let close_sub = iced::window::close_requests().map(Message::WindowCloseRequested);

        if self.capturing.is_none() {
            return close_sub;
        }
        let capture_sub = iced::keyboard::on_key_press(|key, modifiers| {
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
        });
        Subscription::batch([close_sub, capture_sub])
    }
}
