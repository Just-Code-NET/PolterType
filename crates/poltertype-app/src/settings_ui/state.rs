//! `SettingsApp` — the window's whole mutable state.

use std::path::PathBuf;
use std::sync::Arc;

use iced::keyboard::{Key, key::Named};
use iced::widget::text_editor;
use iced::{Subscription, Theme};
use poltertype_core::engine::ModSet;
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
    /// OS dark-mode preference for `[general].ui_theme = "system"`,
    /// sampled once at window start (iced's own auto-detection misses
    /// the XDG portal — see [`super::system_theme`]). Not re-sampled
    /// live: re-detecting would spawn probe processes per render.
    pub(super) system_prefers_dark: bool,
    /// Call counter behind [`SettingsApp::backdrop_color`]. `Cell`
    /// because `view` only gets `&self`.
    pub(super) bg_jitter: std::cell::Cell<u32>,
    /// Whether this session lets a windowless process reach the
    /// clipboard, sampled once when the window opens.
    ///
    /// Probed here rather than read from a list of desktop names: the
    /// answer depends on which Wayland protocols the compositor
    /// actually advertises, and a name is a guess about that. `Err`
    /// carries the sentence the pane shows instead of the toggle.
    pub(super) selection_support: Result<(), String>,
    /// Whether the user has picked a conversion mode *in this window*.
    ///
    /// `[general].paused` belongs to the tray, which rewrites it every
    /// time auto-switch is paused — so a save normally folds the
    /// on-disk value back in rather than the one this window opened
    /// with (issue #46). Clicking the chips is the one case where the
    /// window means it, and this is what tells the two apart.
    pub(super) conversion_chosen_here: bool,
    pub(super) save_banner: Option<SaveBanner>,
    /// `Some(kind)` while the user is in "press a combination…" mode;
    /// the keyboard subscription consults this to decide whether key
    /// events become `HotkeyCaptured` or are ignored.
    pub(super) capturing: Option<HotkeyKind>,
    /// The modifier half of capture: which modifiers this gesture has
    /// held, and whether a single-modifier tap is waiting for its
    /// twin. Only meaningful while `capturing` is `Some`.
    pub(super) mod_capture: ModCapture,
    /// Live answer from the permission probe, re-read on every *Check
    /// again* click. Held rather than probed inside `view`: `view` runs
    /// every frame and this touches the filesystem, and a value that
    /// changes mid-render makes "did my click work?" unanswerable.
    pub(super) setup: poltertype_input::setup::SetupReport,
    /// `None` when no backend could be built — the honest banner for
    /// hooks that are fine while switching is not.
    pub(super) layout_backend: Option<String>,
    /// Feedback for the Setup pane's own buttons, kept apart from the
    /// global save banner.
    pub(super) setup_status: Option<SaveBanner>,
    /// Draft in the Exceptions pane's "add a disabled app" input.
    pub(super) exception_draft: String,
    /// Installed plug-ins that declare a settings pane, with the values
    /// read from *their* config files. Loaded once when the window
    /// opens: those files belong to other programs, and re-reading them
    /// per frame would fight whoever else writes them.
    pub(super) plugins: Vec<super::plugin_pane::PluginPane>,

    // ── Commands pane: draft of a new command ──────────────────────
    /// Free-form display name. Falls back to id if blank at Add time.
    pub(super) command_draft_name: String,
    /// Trigger token the user types to fire this command. Stored
    /// verbatim, validated on the Add path: a `TextInput` with a forced
    /// trim fights common typing patterns. See [`UserCommand::trigger`].
    pub(super) command_draft_trigger: String,
    /// Maps to [`CommandAction`] at Add time using `command_draft_param`.
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
    /// Per-pane status banner, independent of the global save banner.
    pub(super) command_status: Option<SaveBanner>,

    // ── Wordlists pane ─────────────────────────────────────────────
    /// Profile id being edited. Empty = the global overlay
    /// (`<config-dir>/wordlists/<stem>.txt`); anything else picks
    /// `<config-dir>/wordlists/profiles/<id>/<stem>.txt`. Opens on
    /// global — the same baseline the engine uses before any
    /// focus-driven profile swap.
    pub(super) wordlist_profile: String,
    /// `None` until the user clicks a layout button, or defaults to the
    /// first OS-active layout when the pane is first opened.
    pub(super) wordlist_layout: Option<LayoutId>,
    pub(super) wordlist_kind: WordlistKind,
    /// Live editor buffer; `text_editor::Content` owns cursor,
    /// selection and undo stack, fed through `Message::WordlistEdit`.
    pub(super) wordlist_content: text_editor::Content,
    /// Per-pane status line, independent of the global save banner.
    pub(super) wordlist_status: Option<SaveBanner>,
    /// Gates the "discard changes" warning when the user switches
    /// layout / kind without saving.
    pub(super) wordlist_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct SaveBanner {
    pub(super) text: String,
    pub(super) is_error: bool,
}

/// Capture state for a modifier-only chord (issue #32), mirroring what
/// the engine's matcher does with the live key stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModCapture {
    /// Modifier keys down right now.
    pub(super) down: ModSet,
    /// Every modifier seen during this hold — the gesture is judged on
    /// what was held together, not on what is left at the last release.
    pub(super) peak: ModSet,
    /// A single-modifier tap that has landed and is waiting to see
    /// whether a second one follows. One modifier alone is never a
    /// binding, so nothing is committed until it does.
    pub(super) pending_tap: Option<ModSet>,
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
        // on the pane.
        let (initial_layout, initial_text) = match os_layouts.first().cloned() {
            Some(layout) => {
                let text = read_overlay_file_or_empty("", &layout, WordlistKind::Extras);
                (Some(layout), text)
            }
            None => (None, String::new()),
        };

        // Plug-in configs live beside PolterType's config directory,
        // not inside it — a plug-in is a separate program. Computed
        // here because the struct literal below takes `config_path`.
        let plugin_config_root = config_path
            .parent()
            .and_then(|d| d.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();

        let setup = poltertype_input::setup::probe_setup(
            &store.snapshot().updates.local_signing_identity,
        );
        Self {
            settings,
            os_layouts,
            config_path,
            store,
            pane: initial_pane,
            setup,
            layout_backend,
            setup_status: None,
            system_prefers_dark: super::system_theme::system_prefers_dark(),
            bg_jitter: std::cell::Cell::new(0),
            selection_support: poltertype_input::selection_support().map_err(|gap| gap.to_string()),
            conversion_chosen_here: false,
            save_banner: None,
            capturing: None,
            mod_capture: ModCapture::default(),
            exception_draft: String::new(),
            plugins: {
                let data_dir = poltertype_core::resolve_data_dir().unwrap_or_default();
                super::plugin_pane::load_all(
                    poltertype_core::plugins::extensions(&data_dir),
                    &plugin_config_root,
                )
            },
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

    /// The root backdrop colour: the theme's window background with its
    /// blue channel nudged by an epsilon that changes on every call.
    ///
    /// Workaround for iced 0.13's tiny-skia compositor, whose
    /// partial-present path mis-tracks which swapchain buffer holds
    /// which frame — after a palette change the window blinks between
    /// themes and hover repaints can freeze. A full-window quad whose
    /// colour never repeats marks the whole window damaged, so every
    /// present redraws in full. The epsilon cycles a prime 251 steps of
    /// at most 1/1024, far below 8-bit output precision, so rendered
    /// pixels are identical frame to frame.
    ///
    /// Still here on iced 0.14, and deliberately: the two other
    /// tiny-skia workarounds it shipped beside were re-measured and
    /// dropped, but this one guards a fault that only shows as
    /// *flicker during a live palette change*, which no test sees and
    /// a screenshot cannot catch. Removing it on the strength of a
    /// version bump would be guessing. To settle it: change the theme
    /// with the window up, capture rapid `grim` frames and compare
    /// them (`magick compare -metric AE`) with the jitter removed.
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

    /// Always listens for window-close requests, so unsaved wordlist
    /// edits can be flushed before the window goes away. The keyboard
    /// sub is added only in hotkey-capture mode: otherwise every
    /// keystroke in the window would allocate a `Message` and
    /// re-render.
    pub(super) fn subscription(&self) -> Subscription<Message> {
        let close_sub = iced::window::close_requests().map(Message::WindowCloseRequested);

        if self.capturing.is_none() {
            return close_sub;
        }
        // iced 0.14 replaced `on_key_press` / `on_key_release` with a
        // single event stream. `listen_with` takes a plain `fn`, not a
        // closure — which these already were, having nothing to
        // capture: the capture state lives in `self.capturing` and is
        // read by `update`, not here.
        let capture_sub = iced::event::listen_with(|event, _status, _window| {
            let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) = event
            else {
                return None;
            };
            // Swallowing Esc here would read as a frozen UI: it is what
            // people press once they realise they don't want to rebind.
            if matches!(key, Key::Named(Named::Escape)) {
                return Some(Message::HotkeyRebindCancel);
            }
            // A modifier on its own is not yet a combination — it is
            // either one being composed, or the first half of a
            // modifier-only gesture, which only the release settles.
            if let Some(role) = mod_role_of(&key) {
                return Some(Message::HotkeyModifier {
                    role,
                    pressed: true,
                    held: mods_from_iced(modifiers),
                });
            }
            // Single-key hotkeys (`A`, `Space`) would clash with normal
            // typing. Caps Lock is the exception people ask for by name
            // (issue #41): it is bound bare or not at all, and once it
            // has been taken out of the layout — which it has to be, or
            // it latches on every press — it types nothing to clash
            // with. Recognised by its physical code as well as its
            // name, because taking it out of the layout is what stops
            // it having a name.
            if modifiers.is_empty() && !is_capslock(&key, physical_key) {
                return None;
            }
            // What the key *renders* as first, because that is what
            // the user reads off their keycap; the physical code when
            // the rendering is something the reader cannot take back —
            // a Cyrillic letter, or the `§` an Apple ISO keyboard puts
            // left of `Z` (issue #43).
            let combo = format_hotkey(&key, modifiers);
            let combo = if is_usable_hotkey(&combo) {
                combo
            } else {
                physical_hotkey(physical_key, modifiers).unwrap_or(combo)
            };
            Some(Message::HotkeyCaptured(combo))
        });
        // The other half of a modifier-only gesture, and the half that
        // decides it: a chord of modifiers is judged when they come
        // back up. Without this the press above only ever accumulated.
        let release_sub = iced::event::listen_with(|event, _status, _window| {
            let iced::Event::Keyboard(iced::keyboard::Event::KeyReleased {
                key, modifiers, ..
            }) = event
            else {
                return None;
            };
            mod_role_of(&key).map(|role| Message::HotkeyModifier {
                role,
                pressed: false,
                held: mods_from_iced(modifiers),
            })
        });
        Subscription::batch([close_sub, capture_sub, release_sub])
    }
}
