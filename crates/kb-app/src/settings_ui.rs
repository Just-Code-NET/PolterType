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
//! Side-nav with five panes:
//!
//! * **Languages** — checkboxes for every layout the OS reports as
//!   active (queried via `LayoutSwitcher::list_active`). Toggling a
//!   box updates the `[languages].active` allow-list. An **empty**
//!   allow-list means "use every OS-active layout" — the default,
//!   and the UI displays it that way (every box ticked).
//! * **Hotkeys** — current pause / switch-last bindings, plus a
//!   "Rebind" button per row that flips the UI into capture mode
//!   and writes the next valid `<modifier>+<key>` combo back.
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
};
use iced::{Element, Length, Padding, Subscription, Task, Theme};
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
    General,
    Exceptions,
    About,
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
        Self {
            settings,
            os_layouts,
            config_path,
            store,
            pane: Pane::Languages,
            save_banner: None,
            capturing: None,
            exception_draft: String::new(),
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
            .push(mk("General", Pane::General))
            .push(mk("Exceptions", Pane::Exceptions))
            .push(mk("About", Pane::About)),
    )
    .width(180)
    .height(Length::Fill)
    .into()
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
