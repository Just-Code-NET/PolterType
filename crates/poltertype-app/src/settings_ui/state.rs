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
use super::theme;

pub struct SettingsApp {
    pub(super) settings: Settings,
    pub(super) os_layouts: Vec<LayoutId>,
    pub(super) config_path: PathBuf,
    pub(super) store: Arc<SettingsStore>,
    pub(super) pane: Pane,
    /// OS dark-mode preference, sampled once at window start via
    /// [`super::system_theme::system_prefers_dark`] (iced's own
    /// auto-detection misses the XDG portal — see that module). Used
    /// when `[general].ui_theme = "system"`. Not re-sampled live —
    /// the window is short-lived, and re-detecting per frame would
    /// spawn probe processes on every render.
    pub(super) system_prefers_dark: bool,
    /// Call counter consulted by `view` to nudge the root backdrop
    /// colour by an invisible epsilon on every rebuild — see the
    /// backdrop comment in `view` for why. `Cell` because `view`
    /// only gets `&self`.
    pub(super) bg_jitter: std::cell::Cell<u32>,
    /// Set when [`Message::Save`] writes successfully — surfaced as a
    /// transient banner in the footer so the user gets feedback the
    /// click did something.
    pub(super) save_banner: Option<SaveBanner>,
    /// `Some(kind)` while the user is in "press a combination…" mode.
    /// The keyboard subscription consults this to know whether to
    /// route key events to `HotkeyCaptured` or ignore them.
    pub(super) capturing: Option<HotkeyKind>,
    /// Live answer from the permission probe, re-read on every
    /// *Check again* click. Held rather than probed inside `view`
    /// because `view` runs on every frame and this touches the
    /// filesystem — and because a value that changes under the user
    /// mid-render is how a "did my click work?" question becomes
    /// unanswerable.
    pub(super) setup: poltertype_input::setup::SetupReport,
    /// Name of the layout-switcher backend, or `None` when no backend
    /// could be built. The honest banner for the case the hooks are
    /// fine and switching is not.
    pub(super) layout_backend: Option<String>,
    /// Feedback for the Setup pane's own buttons ("Copied.", "Nothing
    /// changed yet."), kept apart from the global save banner.
    pub(super) setup_status: Option<SaveBanner>,
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
        initial_pane: Pane,
        layout_backend: Option<String>,
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
            pane: initial_pane,
            setup: poltertype_input::setup::probe_setup(),
            layout_backend,
            setup_status: None,
            system_prefers_dark: super::system_theme::system_prefers_dark(),
            bg_jitter: std::cell::Cell::new(0),
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
        format!("PolterType · Settings ({})", self.config_path.display())
    }

    /// The user's theme preference, parsed fresh from the staged
    /// settings so the segmented picker on the General pane applies
    /// instantly (before any Save).
    pub(super) fn theme_choice(&self) -> ThemeChoice {
        ThemeChoice::from_config(&self.settings.general.ui_theme)
    }

    pub(super) fn theme(&self) -> Theme {
        // Branded light / dark themes built from the same design
        // tokens as poltertype.com; `System` follows the OS
        // preference sampled at window start.
        let dark = match self.theme_choice() {
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
            ThemeChoice::System => self.system_prefers_dark,
        };
        if dark { theme::dark() } else { theme::light() }
    }

    /// Brand tokens for the active theme — view code colours text
    /// (`.color(app.brand().muted)`) without threading `&Theme`
    /// through every helper.
    pub(super) fn brand(&self) -> &'static theme::BrandPalette {
        theme::brand_palette(&self.theme())
    }

    /// The root backdrop colour for this view rebuild: the theme's
    /// window background with its blue channel nudged by an epsilon
    /// that changes on every call.
    ///
    /// The nudge is a deliberate workaround for iced 0.13's tiny-skia
    /// compositor. Its partial-present path mis-tracks which swapchain
    /// buffer holds which frame, so small damage regions get painted
    /// onto stale buffers — after a palette change the window blinks
    /// between the new theme and an old-theme frame, and hover
    /// repaints can freeze outright. A full-window quad whose colour
    /// never repeats makes the layer diff mark the whole window
    /// damaged on every UI change, so every present redraws the full
    /// frame and stale buffers can't survive.
    ///
    /// The epsilon cycles through 251 steps of at most 1/1024 — far
    /// below 8-bit output precision, so the rendered pixels are
    /// identical frame to frame. A prime cycle length keeps
    /// consecutive rebuilds distinct regardless of how many times the
    /// runtime samples the view. The window repaints only on input
    /// events, so the extra fill is negligible. Remove when the
    /// workspace moves to iced 0.14.
    pub(super) fn backdrop_color(&self) -> iced::Color {
        let bg = self.brand().bg;
        let n = self.bg_jitter.get().wrapping_add(1);
        self.bg_jitter.set(n);
        let jitter = (n % 251) as f32 / (251.0 * 1024.0);
        iced::Color {
            b: (bg.b + jitter).min(1.0),
            ..bg
        }
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
