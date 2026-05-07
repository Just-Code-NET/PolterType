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
//! Side-nav with three panes (kept minimal — power users still edit
//! `config.toml` directly via the tray's "Open Settings" entry):
//!
//! * **Languages** — checkboxes for every layout the OS reports as
//!   active (queried via `LayoutSwitcher::list_active`). Toggling a
//!   box updates the `[languages].active` allow-list. An **empty**
//!   allow-list means "use every OS-active layout" — the default.
//! * **General** — the boolean / numeric knobs from
//!   `GeneralSettings` + `EngineSettings`: autostart, sound on
//!   correction, suppress-in-identifiers, idle timeout.
//! * **About** — version + repo links. The bottom row also exposes
//!   a "Reset to defaults" button as a power-user escape hatch.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use iced::widget::{Button, Checkbox, Column, Container, Row, Scrollable, Space, Text, button};
use iced::{Element, Length, Padding, Task, Theme};
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
    General,
    About,
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
                let list = &mut self.settings.languages.active;
                if active {
                    if !list.contains(&id) {
                        list.push(id);
                    }
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
            Pane::General => self.view_general(),
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
        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Languages").size(24))
            .push(
                Text::new(
                    "Pick which OS-active layouts kb-switcher should consider when \
                     deciding whether to switch. Leaving every box unchecked means \
                     'use all OS-active layouts' — the default.",
                )
                .size(13),
            );

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
                let is_active = self.settings.languages.active.contains(id);
                let is_ignored = self.settings.languages.ignored.contains(id);
                let row = Row::new()
                    .spacing(16)
                    .push(Text::new(id.as_str().to_string()).width(Length::FillPortion(2)))
                    .push(
                        Checkbox::new("Active", is_active)
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
                "Tip: 'Active' is an allow-list — empty means everything passes. \
                 'Ignore' is a hard veto and always wins.",
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
            .push(mk("General", Pane::General))
            .push(mk("About", Pane::About)),
    )
    .width(180)
    .height(Length::Fill)
    .into()
}
