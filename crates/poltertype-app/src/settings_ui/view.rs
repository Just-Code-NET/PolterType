//! Widget tree construction: the `view` half of the iced loop.

use iced::widget::{
    Button, Checkbox, Column, Container, Row, Scrollable, Space, Text, TextInput, button,
    text_editor,
};
use iced::{Element, Length, Padding, Theme};

use super::enums::*;
use super::helpers::*;
use super::state::*;

impl SettingsApp {
    pub(super) fn view(&self) -> Element<'_, Message> {
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

    pub(super) fn view_languages(&self) -> Element<'_, Message> {
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
                     Untick a box to restrict poltertype to a subset.",
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

    pub(super) fn view_hotkeys(&self) -> Element<'_, Message> {
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

    pub(super) fn view_commands(&self) -> Element<'_, Message> {
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

    pub(super) fn view_wordlists(&self) -> Element<'_, Message> {
        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Wordlists").size(24))
            .push(
                Text::new(
                    "Add language-specific words to the per-layout dictionary \
                     overlay. Use the Save button below to persist your edits, \
                     or just close the window — either way, the engine's \
                     dictionary set refreshes so new words start counting \
                     toward detection on the next typed word, no tray \
                     restart needed.",
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

        // Per-pane Save / Reload buttons were removed in beta.12 —
        // the single footer Save+Reload pair now covers everything
        // (config.toml + the active wordlist edit) for a less
        // ambiguous UI. Dirty marker + status banner stay so the
        // user still sees "unsaved changes" + auto-save outcomes
        // from layout/profile/kind switches.
        col = col.push(
            Row::new()
                .spacing(8)
                .push(dirty_marker)
                .push(Space::with_width(Length::Fill))
                .push(status),
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

    pub(super) fn view_exceptions(&self) -> Element<'_, Message> {
        let mut col = Column::new()
            .spacing(12)
            .push(Text::new("Exceptions").size(24))
            .push(
                Text::new(
                    "Poltertype skips auto-correction when the foreground app's \
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

    pub(super) fn view_general(&self) -> Element<'_, Message> {
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
                    "Show a 2-second system notification on auto-switch",
                    g.show_notifications,
                )
                .on_toggle(Message::ShowNotificationsToggled),
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

    pub(super) fn view_about(&self) -> Element<'_, Message> {
        Column::new()
            .spacing(10)
            .push(Text::new("About poltertype").size(24))
            .push(Text::new(format!("Version {}", env!("CARGO_PKG_VERSION"))))
            .push(Text::new(
                "Cross-platform automatic keyboard layout switcher.",
            ))
            .push(Space::with_height(8))
            .push(Text::new("Project: https://github.com/Just-Code-NET/poltertype").size(13))
            .push(Text::new("Issues: https://github.com/Just-Code-NET/poltertype/issues").size(13))
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

    pub(super) fn view_footer(&self) -> Element<'_, Message> {
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

pub fn nav_panel(active: Pane) -> Element<'static, Message> {
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
            .push(Text::new("Poltertype").size(18))
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
