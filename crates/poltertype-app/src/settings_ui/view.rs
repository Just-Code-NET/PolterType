//! Widget tree construction: the `view` half of the iced loop.
//!
//! Visual language mirrors poltertype.com: a surface-coloured sidebar
//! with the GhostMark + wordmark, content grouped into hairline
//! cards, hotkeys rendered as physical keycap chips, brand-indigo
//! primary actions. All colours come from
//! [`theme::BrandPalette`](super::theme::BrandPalette) via
//! `self.brand()` so every pane re-themes with the window.

use iced::widget::{
    Button, Canvas, Checkbox, Column, Container, Row, Scrollable, Space, Text, TextInput,
    container, horizontal_rule, text_editor, vertical_rule,
};
use iced::{Alignment, Element, Font, Length, Padding};

use super::consts::*;
use super::enums::*;
use super::helpers::*;
use super::state::*;
use super::theme::{self, FONT_BOLD, GhostMark};

impl SettingsApp {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let body = match self.pane {
            Pane::Languages => self.view_languages(),
            Pane::Hotkeys => self.view_hotkeys(),
            Pane::Commands => self.view_commands(),
            Pane::Wordlists => self.view_wordlists(),
            Pane::General => self.view_general(),
            Pane::Exceptions => self.view_exceptions(),
            Pane::About => self.view_about(),
        };

        let content = Column::new()
            .push(
                Scrollable::new(Container::new(body).padding(Padding {
                    top: 22.0,
                    right: 24.0,
                    bottom: 16.0,
                    left: 24.0,
                }))
                .height(Length::Fill)
                .width(Length::Fill),
            )
            .push(self.view_footer());

        let main = Row::new()
            .push(self.nav_panel())
            .push(vertical_rule(1).style(theme::hairline))
            .push(
                Container::new(content)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .height(Length::Fill);

        // Root backdrop quad. The colour is the theme's window
        // background nudged by an invisible per-rebuild epsilon —
        // NOT cosmetic, it defeats buggy partial presents in iced
        // 0.13's tiny-skia compositor. See
        // [`SettingsApp::backdrop_color`].
        let backdrop = self.backdrop_color();
        Container::new(main)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(backdrop)),
                ..container::Style::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Branded side navigation: mark + wordmark on top, pane list,
    /// version pinned to the bottom.
    fn nav_panel(&self) -> Element<'_, Message> {
        let b = self.brand();
        let item = |label: &'static str, pane: Pane| -> Element<'static, Message> {
            Button::new(Text::new(label).size(13))
                .on_press(Message::SelectPane(pane))
                .style(theme::nav(self.pane == pane))
                .width(Length::Fill)
                .padding(Padding {
                    top: 7.0,
                    right: 12.0,
                    bottom: 7.0,
                    left: 12.0,
                })
                .into()
        };

        let brand_row = Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(Canvas::new(GhostMark).width(32).height(32))
            .push(
                Column::new()
                    .push(
                        Text::new("PolterType")
                            .size(16)
                            .font(FONT_BOLD)
                            .color(b.ink),
                    )
                    .push(Text::new("Settings").size(11).color(b.muted)),
            );

        Container::new(
            Column::new()
                .spacing(3)
                .padding(14)
                .push(Container::new(brand_row).padding(Padding {
                    top: 2.0,
                    right: 0.0,
                    bottom: 14.0,
                    left: 2.0,
                }))
                .push(item("Languages", Pane::Languages))
                .push(item("Hotkeys", Pane::Hotkeys))
                .push(item("Commands", Pane::Commands))
                .push(item("Wordlists", Pane::Wordlists))
                .push(item("General", Pane::General))
                .push(item("Exceptions", Pane::Exceptions))
                .push(item("About", Pane::About))
                .push(Space::with_height(Length::Fill))
                .push(
                    Text::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .size(11)
                        .color(b.muted),
                ),
        )
        .width(190)
        .height(Length::Fill)
        .style(theme::sidebar)
        .into()
    }

    pub(super) fn view_languages(&self) -> Element<'_, Message> {
        let b = self.brand();
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

        // Lead with WHERE the list comes from — first-run users read
        // "en-US / uk-UA" as a product default and go looking for an
        // "add language" button that (deliberately) doesn't exist:
        // PolterType follows the OS keyboard configuration instead of
        // keeping a second list to drift out of sync.
        let subtitle = "This list mirrors the keyboard layouts enabled in your \
             operating system. To add or remove a language, change your \
             system's keyboard settings, then reopen this window."
            .to_owned();

        let status = if implicit_all {
            "All of them are currently considered. Untick 'Active' to \
             restrict PolterType to a subset."
                .to_owned()
        } else {
            format!(
                "Restricted to {} layout(s). Tick more to include them, \
                 or hit 'Reset to defaults' on the About pane to go back \
                 to 'use every OS layout'.",
                allow_list.len()
            )
        };

        let mut col = Column::new()
            .spacing(14)
            .push(pane_header(b, "Languages", subtitle))
            .push(Text::new(status).size(12).color(b.muted));

        if self.os_layouts.is_empty() {
            col = col.push(card(
                Text::new(
                    "No OS layouts detected. Add languages in your system's keyboard \
                     settings, then reopen this window.",
                )
                .size(13)
                .color(b.muted),
            ));
        } else {
            let mut rows = Column::new().spacing(10);
            for id in &self.os_layouts {
                let is_active_effective = implicit_all || allow_list.contains(id);
                let is_ignored = self.settings.languages.ignored.contains(id);
                rows = rows.push(
                    Row::new()
                        .spacing(16)
                        .align_y(Alignment::Center)
                        .push(
                            Text::new(id.as_str().to_string())
                                .size(13)
                                .font(Font::MONOSPACE)
                                .width(Length::FillPortion(2)),
                        )
                        .push(
                            Checkbox::new("Active", is_active_effective)
                                .text_size(13)
                                .on_toggle({
                                    let id = id.clone();
                                    move |flag| Message::LanguageToggled(id.clone(), flag)
                                })
                                .width(Length::FillPortion(1)),
                        )
                        .push(
                            Checkbox::new("Ignore", is_ignored)
                                .text_size(13)
                                .on_toggle({
                                    let id = id.clone();
                                    move |flag| Message::LanguageIgnoreToggled(id.clone(), flag)
                                })
                                .width(Length::FillPortion(1)),
                        ),
                );
            }
            col = col.push(card(rows));
        }

        col.push(tip(
            b,
            "Tip: 'Active' is the allow-list — when nothing is restricted \
             every OS layout is included. 'Ignore' is a hard veto and \
             always wins.",
        ))
        .into()
    }

    pub(super) fn view_hotkeys(&self) -> Element<'_, Message> {
        let b = self.brand();
        let row = |label: &'static str, current: &str, kind: HotkeyKind| -> Element<'_, Message> {
            let capturing = self.capturing == Some(kind);
            let display: Element<'_, Message> = if capturing {
                Text::new("Press a combination… (Esc to cancel)")
                    .size(13)
                    .color(b.warn)
                    .into()
            } else {
                hotkey_chips(b, current)
            };
            let action = if capturing {
                Button::new(Text::new("Cancel").size(12)).on_press(Message::HotkeyRebindCancel)
            } else {
                Button::new(Text::new("Rebind").size(12)).on_press(Message::HotkeyRebindStart(kind))
            };
            Row::new()
                .spacing(16)
                .align_y(Alignment::Center)
                .push(Text::new(label).size(13).width(Length::FillPortion(2)))
                .push(Container::new(display).width(Length::FillPortion(3)))
                .push(action.style(theme::secondary).padding(Padding {
                    top: 5.0,
                    right: 12.0,
                    bottom: 5.0,
                    left: 12.0,
                }))
                .into()
        };

        Column::new()
            .spacing(14)
            .push(pane_header(
                b,
                "Hotkeys",
                "Global hotkeys are registered with the OS at startup. \
                 Click 'Rebind', press the new combination, then save. \
                 The new binding takes effect after the tray restarts \
                 (Save, then Quit and relaunch from the tray)."
                    .to_owned(),
            ))
            .push(card(
                Column::new()
                    .spacing(12)
                    .push(row(
                        "Pause / resume auto-switch",
                        &self.settings.hotkeys.pause_toggle,
                        HotkeyKind::Pause,
                    ))
                    .push(row(
                        "Force-switch the last word",
                        &self.settings.hotkeys.manual_switch_last,
                        HotkeyKind::SwitchLast,
                    )),
            ))
            .push(tip(
                b,
                "Tip: capture refuses single-letter combinations and bare \
                 keys — at least one of Ctrl / Alt / Shift / Cmd is required. \
                 Esc cancels capture without changing anything.",
            ))
            .into()
    }

    pub(super) fn view_commands(&self) -> Element<'_, Message> {
        let b = self.brand();
        let mut col = Column::new().spacing(14).push(pane_header(
            b,
            "Commands",
            "Type a short token, get a phrase — like classic snippet expanders. \
             For example: typing the trigger `anrl` + space expands into \
             `Anatomical Reference List `. The engine watches every word \
             boundary and fires when the typed token matches. Pause / \
             switch-last live separately on the Hotkeys pane. New commands \
             take effect after Save + restart."
                .to_owned(),
        ));

        // ── Existing commands list ──────────────────────────────────
        if self.settings.commands.is_empty() {
            col = col.push(card(
                Text::new("No commands yet — fill the form below to add one.")
                    .size(12)
                    .color(b.muted),
            ));
        } else {
            let mut rows = Column::new().spacing(10);
            for (idx, cmd) in self.settings.commands.iter().enumerate() {
                let summary = format_command_summary(cmd);
                rows = rows.push(
                    Row::new()
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .push(
                            Container::new(keycap_chip(cmd.trigger.clone()))
                                .width(Length::FillPortion(2)),
                        )
                        .push(
                            Text::new(summary)
                                .size(12)
                                .color(b.muted)
                                .width(Length::FillPortion(5)),
                        )
                        .push(
                            Button::new(Text::new("×").size(14))
                                .on_press(Message::CommandRemove(idx))
                                .style(theme::danger_icon)
                                .padding(Padding {
                                    top: 2.0,
                                    right: 8.0,
                                    bottom: 2.0,
                                    left: 8.0,
                                }),
                        ),
                );
            }
            col = col.push(card(rows));
        }

        // ── "Add new command" form ──────────────────────────────────
        let label = |text: &'static str| -> Element<'static, Message> {
            Text::new(text)
                .size(12)
                .color(b.muted)
                .width(Length::FillPortion(1))
                .into()
        };

        let mut form = Column::new()
            .spacing(10)
            .push(section_title(b, "Add a new command"));

        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label("Name"))
                .push(
                    TextInput::new("e.g. Insert email signature", &self.command_draft_name)
                        .on_input(Message::CommandDraftNameChanged)
                        .style(theme::input)
                        .size(13)
                        .width(Length::FillPortion(4)),
                ),
        );

        // Trigger row: text input for the typed token (e.g. `anrl`).
        // The buffer resets at every word boundary, so triggers must
        // be a single token — the validation path on Add will refuse
        // any whitespace.
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label("Trigger"))
                .push(
                    TextInput::new("e.g. anrl, ;sig, ((en))", &self.command_draft_trigger)
                        .on_input(Message::CommandDraftTriggerChanged)
                        .style(theme::input)
                        .size(13)
                        .width(Length::FillPortion(4)),
                ),
        );

        // Action kind picker (radio-style chips).
        let mk_kind_btn = |kind: CommandActionKind| -> Element<'_, Message> {
            Button::new(Text::new(kind.label()).size(12))
                .on_press(Message::CommandDraftActionKindChanged(kind))
                .style(theme::chip(self.command_draft_action_kind == kind))
                .padding(Padding {
                    top: 5.0,
                    right: 10.0,
                    bottom: 5.0,
                    left: 10.0,
                })
                .into()
        };
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label("Action"))
                .push(
                    Row::new()
                        .spacing(6)
                        .push(mk_kind_btn(CommandActionKind::TypeText))
                        .push(mk_kind_btn(CommandActionKind::SwitchLayout))
                        .push(mk_kind_btn(CommandActionKind::OpenPath))
                        .width(Length::FillPortion(4)),
                ),
        );

        // Param input (placeholder swaps based on action kind).
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label(match self.command_draft_action_kind {
                    CommandActionKind::TypeText => "Text",
                    CommandActionKind::SwitchLayout => "Layout id",
                    CommandActionKind::OpenPath => "Path / URL",
                }))
                .push(
                    TextInput::new(
                        self.command_draft_action_kind.placeholder(),
                        &self.command_draft_param,
                    )
                    .on_input(Message::CommandDraftParamChanged)
                    .style(theme::input)
                    .size(13)
                    .width(Length::FillPortion(4)),
                ),
        );

        // Optional apps filter.
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label("Apps (optional)"))
                .push(
                    TextInput::new(
                        "comma-separated, e.g. Code.exe,idea64.exe",
                        &self.command_draft_apps,
                    )
                    .on_input(Message::CommandDraftAppsChanged)
                    .on_submit(Message::CommandAdd)
                    .style(theme::input)
                    .size(13)
                    .width(Length::FillPortion(4)),
                ),
        );

        // Status + Add row.
        let status: Element<'_, Message> = match &self.command_status {
            Some(banner) => status_line(b, banner),
            None => Space::with_width(Length::Shrink).into(),
        };
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(status)
                .push(Space::with_width(Length::Fill))
                .push(
                    Button::new(Text::new("Add command").size(12))
                        .on_press(Message::CommandAdd)
                        .style(theme::primary)
                        .padding(Padding {
                            top: 6.0,
                            right: 14.0,
                            bottom: 6.0,
                            left: 14.0,
                        }),
                ),
        );

        col.push(card(form))
            .push(tip(
                b,
                "Tips: pick triggers that don't collide with words you actually type — \
                 `the` would expand on every English sentence; `;sig` or `((email))` \
                 are safer. Match is exact and case-sensitive. Leave 'Apps' empty for \
                 a global command, or list `OUTLOOK.EXE,thunderbird.exe` to scope a \
                 command (case-insensitive basename match).",
            ))
            .into()
    }

    pub(super) fn view_wordlists(&self) -> Element<'_, Message> {
        let b = self.brand();
        let mut col = Column::new().spacing(14).push(pane_header(
            b,
            "Wordlists",
            "Add language-specific words to the per-layout dictionary \
             overlay. Use the Save button below to persist your edits, \
             or just close the window — either way, the engine's \
             dictionary set refreshes so new words start counting \
             toward detection on the next typed word, no tray \
             restart needed."
                .to_owned(),
        ));

        if self.os_layouts.is_empty() {
            return col
                .push(card(
                    Text::new(
                        "No OS layouts detected. Add languages in your system's \
                         keyboard settings, then reopen this window.",
                    )
                    .size(13)
                    .color(b.muted),
                ))
                .into();
        }

        let picker_label = |text: &'static str| -> Element<'static, Message> {
            Text::new(text)
                .size(12)
                .color(b.muted)
                .width(Length::Fixed(52.0))
                .into()
        };

        let mut pickers = Column::new().spacing(8);

        // ── Profile picker (Global + each configured profile) ──────
        // Only shown when the user has at least one profile configured;
        // otherwise the row would be a redundant single "Global"
        // button. Add profiles via `[[wordlists.profiles]]` in
        // config.toml — full profile-list management UI is queued for
        // a follow-up.
        if !self.settings.wordlists.profiles.is_empty() {
            let profile_btn = |id: &str, pick_label: &str| -> Element<'_, Message> {
                Button::new(Text::new(pick_label.to_owned()).size(12))
                    .on_press(Message::WordlistProfileSelected(id.to_owned()))
                    .style(theme::chip(self.wordlist_profile == id))
                    .padding(Padding {
                        top: 4.0,
                        right: 10.0,
                        bottom: 4.0,
                        left: 10.0,
                    })
                    .into()
            };
            let mut profile_row = Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(picker_label("Profile"));
            profile_row = profile_row.push(profile_btn("", "Global"));
            for p in &self.settings.wordlists.profiles {
                let pick_label = if p.name.is_empty() {
                    p.id.clone()
                } else {
                    p.name.clone()
                };
                profile_row = profile_row.push(profile_btn(&p.id, &pick_label));
            }
            pickers = pickers.push(profile_row);
        }

        // ── Layout picker (one chip per OS-active layout) ───────────
        let mut layout_row = Row::new()
            .spacing(6)
            .align_y(Alignment::Center)
            .push(picker_label("Layout"));
        for id in &self.os_layouts {
            layout_row = layout_row.push(
                Button::new(Text::new(id.as_str().to_string()).size(12))
                    .on_press(Message::WordlistLayoutSelected(id.clone()))
                    .style(theme::chip(self.wordlist_layout.as_ref() == Some(id)))
                    .padding(Padding {
                        top: 4.0,
                        right: 10.0,
                        bottom: 4.0,
                        left: 10.0,
                    }),
            );
        }
        pickers = pickers.push(layout_row);

        // ── Kind picker (Extras vs Stop) ────────────────────────────
        let kind_button = |kind: WordlistKind| -> Element<'_, Message> {
            Button::new(Text::new(kind.label()).size(12))
                .on_press(Message::WordlistKindSelected(kind))
                .style(theme::chip(self.wordlist_kind == kind))
                .padding(Padding {
                    top: 4.0,
                    right: 10.0,
                    bottom: 4.0,
                    left: 10.0,
                })
                .into()
        };
        pickers = pickers.push(
            Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(picker_label("List"))
                .push(kind_button(WordlistKind::Extras))
                .push(kind_button(WordlistKind::Stop)),
        );

        col = col.push(pickers);

        // ── Resolved-path hint ──────────────────────────────────────
        if let Some(id) = &self.wordlist_layout {
            let path_label =
                match resolve_overlay_path(&self.wordlist_profile, id, self.wordlist_kind) {
                    Some(p) => p.display().to_string(),
                    None => "(no config dir resolved on this platform)".to_owned(),
                };
            col = col.push(
                Text::new(format!("File: {path_label}"))
                    .size(11)
                    .font(Font::MONOSPACE)
                    .color(b.muted),
            );
        }

        // ── Editor body + status row ────────────────────────────────
        let editor: Element<'_, Message> = if self.wordlist_layout.is_some() {
            text_editor(&self.wordlist_content)
                .on_action(Message::WordlistEdit)
                .height(Length::Fixed(240.0))
                .padding(10)
                .font(Font::MONOSPACE)
                .style(theme::editor)
                .placeholder("# one word per line — '#' starts a comment\n")
                .into()
        } else {
            Text::new("Pick a layout above to start editing.")
                .size(13)
                .color(b.muted)
                .into()
        };
        col = col.push(editor);

        let dirty_marker: Element<'_, Message> = if self.wordlist_dirty {
            // Plain text, no bullet glyph — the default UI font on a
            // clean Linux install may lack it and render tofu.
            Text::new("unsaved changes").size(11).color(b.warn).into()
        } else {
            Space::with_width(Length::Shrink).into()
        };
        let status: Element<'_, Message> = match &self.wordlist_status {
            Some(banner) => status_line(b, banner),
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

        col.push(tip(
            b,
            "Tip: Extras helps detection prefer your jargon, \
             project nouns or family names. Stop list extends the \
             1- / 2-letter entries the detector accepts as real \
             words instead of typos.",
        ))
        .into()
    }

    pub(super) fn view_exceptions(&self) -> Element<'_, Message> {
        let b = self.brand();
        let col = Column::new().spacing(14).push(pane_header(
            b,
            "Exceptions",
            "PolterType skips auto-correction when the foreground app's \
             executable basename is in this list. Manual switch (the \
             hotkey on the Hotkeys pane) bypasses the list — devs can \
             still fix wrong-layout identifiers explicitly inside an IDE."
                .to_owned(),
        ));

        let mut rows = Column::new().spacing(8);
        if self.settings.exceptions.disabled_apps.is_empty() {
            rows = rows.push(
                Text::new("No exceptions — PolterType is active in every app.")
                    .size(12)
                    .color(b.muted),
            );
        }
        for (idx, entry) in self.settings.exceptions.disabled_apps.iter().enumerate() {
            rows = rows.push(
                Row::new()
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .push(
                        Text::new(entry.clone())
                            .size(13)
                            .font(Font::MONOSPACE)
                            .width(Length::Fill),
                    )
                    .push(
                        Button::new(Text::new("×").size(14))
                            .on_press(Message::ExceptionRemove(idx))
                            .style(theme::danger_icon)
                            .padding(Padding {
                                top: 2.0,
                                right: 8.0,
                                bottom: 2.0,
                                left: 8.0,
                            }),
                    ),
            );
        }

        col.push(card(rows))
            .push(
                Row::new()
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(
                        TextInput::new("e.g. mygame.exe", &self.exception_draft)
                            .on_input(Message::ExceptionDraftChanged)
                            .on_submit(Message::ExceptionAdd)
                            .style(theme::input)
                            .size(13)
                            .width(Length::Fill),
                    )
                    .push(
                        Button::new(Text::new("Add").size(13))
                            .on_press(Message::ExceptionAdd)
                            .style(theme::primary)
                            .padding(Padding {
                                top: 6.0,
                                right: 14.0,
                                bottom: 6.0,
                                left: 14.0,
                            }),
                    ),
            )
            .push(tip(
                b,
                "Match is case-insensitive against the basename — both \
                 `code.exe` and `Code.exe` work.",
            ))
            .into()
    }

    pub(super) fn view_general(&self) -> Element<'_, Message> {
        let b = self.brand();
        let g = &self.settings.general;
        let e = &self.settings.engine;

        let behaviour = Column::new()
            .spacing(12)
            .push(section_title(b, "Behaviour"))
            .push(
                Checkbox::new("Start automatically when I sign in", g.autostart)
                    .text_size(13)
                    .on_toggle(Message::AutostartToggled),
            )
            .push(
                Checkbox::new("Play a soft chime on correction", g.sound_on_correct)
                    .text_size(13)
                    .on_toggle(Message::SoundOnCorrectToggled),
            )
            .push(
                Checkbox::new(
                    "Show a 2-second system notification on auto-switch",
                    g.show_notifications,
                )
                .text_size(13)
                .on_toggle(Message::ShowNotificationsToggled),
            )
            .push(
                Checkbox::new(
                    "Skip auto-switch on identifiers (foo_bar, snake_case, …)",
                    e.suppress_in_identifiers,
                )
                .text_size(13)
                .on_toggle(Message::SuppressInIdentifiersToggled),
            )
            .push(
                Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(Text::new("Idle timeout (ms):").size(13))
                    .push(
                        Button::new(Text::new("-100").size(12))
                            .on_press(Message::IdleTimeoutDelta(-100))
                            .style(theme::secondary)
                            .padding(Padding {
                                top: 4.0,
                                right: 8.0,
                                bottom: 4.0,
                                left: 8.0,
                            }),
                    )
                    .push(
                        Text::new(format!("{:>5}", e.idle_timeout_ms))
                            .size(13)
                            .font(Font::MONOSPACE),
                    )
                    .push(
                        Button::new(Text::new("+100").size(12))
                            .on_press(Message::IdleTimeoutDelta(100))
                            .style(theme::secondary)
                            .padding(Padding {
                                top: 4.0,
                                right: 8.0,
                                bottom: 4.0,
                                left: 8.0,
                            }),
                    )
                    .push(
                        Text::new("Buffer is cleared after this much keyboard silence.")
                            .size(11)
                            .color(b.muted),
                    ),
            );

        // Theme picker applies instantly (no Save needed to preview);
        // the choice persists to `[general].ui_theme` via footer Save.
        let mut theme_row = Row::new().spacing(6);
        for choice in ThemeChoice::ALL {
            theme_row = theme_row.push(
                Button::new(Text::new(choice.label()).size(12))
                    .on_press(Message::ThemeChoiceChanged(choice))
                    .style(theme::chip(self.theme_choice() == choice))
                    .padding(Padding {
                        top: 5.0,
                        right: 12.0,
                        bottom: 5.0,
                        left: 12.0,
                    }),
            );
        }
        let appearance = Column::new()
            .spacing(12)
            .push(section_title(b, "Appearance"))
            .push(theme_row)
            .push(
                Text::new("System follows the OS light/dark preference. Save to persist.")
                    .size(11)
                    .color(b.muted),
            );

        // Updates. This pane is the app's disclosure surface for the one
        // thing it does that reaches outside the machine, so it names
        // the exact URL rather than saying "checks for updates" and
        // leaving the user to trust us. The interval row is only
        // interactive while updates are on — greying it out is how the
        // UI says "this number means nothing right now" without a
        // second sentence of explanation.
        let u = &self.settings.updates;
        // `on_press` only when updates are on: an iced Button with no
        // handler renders disabled, which is the whole signal we want
        // here — with checking switched off, the interval is a number
        // that means nothing.
        let step = |label: &'static str, delta: i64| {
            let btn = Button::new(Text::new(label).size(12))
                .style(theme::secondary)
                .padding(Padding {
                    top: 4.0,
                    right: 8.0,
                    bottom: 4.0,
                    left: 8.0,
                });
            if u.enabled {
                btn.on_press(Message::UpdateIntervalDelta(delta))
            } else {
                btn
            }
        };
        let interval_row = Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(Text::new("Check every (hours):").size(13))
            .push(step("-1", -1))
            .push(
                Text::new(format!("{:>3}", u.check_interval_hours))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(if u.enabled { b.ink } else { b.muted }),
            )
            .push(step("+1", 1));

        let updates = Column::new()
            .spacing(12)
            .push(section_title(b, "Updates"))
            .push(
                Checkbox::new(
                    "Download new versions automatically, install on restart",
                    u.enabled,
                )
                .text_size(13)
                .on_toggle(Message::AutoUpdateToggled),
            )
            .push(interval_row)
            .push(
                Text::new(
                    "This is the only network connection PolterType makes. It fetches a small \
                     version file from GitHub — no account, no identifier, nothing about you or \
                     what you type. A new version is downloaded, checksum-verified and then left \
                     alone until you quit or click \"Restart to update\" in the tray. Never \
                     installed while you're typing.",
                )
                .size(11)
                .color(b.muted),
            )
            .push(
                Text::new(poltertype_update::MANIFEST_URL)
                    .size(10)
                    .font(Font::MONOSPACE)
                    .color(b.muted),
            );

        let folders = Column::new()
            .spacing(12)
            .push(section_title(b, "Folders"))
            .push(
                Row::new()
                    .spacing(8)
                    .push(folder_button("Open config.toml", Message::OpenConfigFile))
                    .push(folder_button("Logs", Message::OpenLogsDir))
                    .push(folder_button("User wordlists", Message::OpenWordlistsDir))
                    .push(folder_button("User layouts", Message::OpenLayoutsDir)),
            );

        Column::new()
            .spacing(14)
            .push(pane_header(
                b,
                "General",
                "Behaviour of the tray app and the correction engine.".to_owned(),
            ))
            .push(card(behaviour))
            .push(card(appearance))
            .push(card(updates))
            .push(card(folders))
            .into()
    }

    pub(super) fn view_about(&self) -> Element<'_, Message> {
        let b = self.brand();

        let hero = Column::new()
            .spacing(6)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .push(Space::with_height(10))
            .push(Canvas::new(GhostMark).width(64).height(64))
            .push(Space::with_height(4))
            .push(
                Text::new("PolterType")
                    .size(24)
                    .font(FONT_BOLD)
                    .color(b.ink),
            )
            .push(
                Text::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                    .size(12)
                    .color(b.muted),
            )
            .push(Text::new("Cross-platform automatic keyboard layout switcher.").size(13))
            .push(
                Row::new()
                    .spacing(4)
                    .push(link_button("poltertype.com", SITE_URL))
                    .push(link_button("GitHub", REPO_URL))
                    .push(link_button("Report an issue", ISSUES_URL)),
            )
            .push(Space::with_height(6));

        let escape_hatches = Column::new()
            .spacing(12)
            .push(section_title(b, "Power-user escape hatches"))
            .push(
                Row::new()
                    .spacing(8)
                    .push(
                        Button::new(Text::new("Reset to defaults").size(13))
                            .on_press(Message::ResetDefaults)
                            .style(theme::danger)
                            .padding(Padding {
                                top: 6.0,
                                right: 12.0,
                                bottom: 6.0,
                                left: 12.0,
                            }),
                    )
                    .push(
                        Button::new(Text::new("Reload from disk").size(13))
                            .on_press(Message::Reload)
                            .style(theme::secondary)
                            .padding(Padding {
                                top: 6.0,
                                right: 12.0,
                                bottom: 6.0,
                                left: 12.0,
                            }),
                    ),
            )
            .push(
                Text::new(format!("Config: {}", self.config_path.display()))
                    .size(11)
                    .font(Font::MONOSPACE)
                    .color(b.muted),
            );

        Column::new()
            .spacing(14)
            .push(card(hero))
            .push(card(escape_hatches))
            .into()
    }

    pub(super) fn view_footer(&self) -> Element<'_, Message> {
        let b = self.brand();
        let banner: Element<'_, Message> = match &self.save_banner {
            Some(banner) => Text::new(&banner.text)
                .size(12)
                .color(if banner.is_error { b.garble } else { b.ecto })
                .into(),
            None => Space::with_width(Length::Shrink).into(),
        };

        Column::new()
            .push(horizontal_rule(1).style(theme::hairline))
            .push(
                Row::new()
                    .padding(Padding {
                        top: 12.0,
                        right: 24.0,
                        bottom: 14.0,
                        left: 24.0,
                    })
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(banner)
                    .push(Space::with_width(Length::Fill))
                    .push(
                        Button::new(Text::new("Reload").size(13))
                            .on_press(Message::Reload)
                            .style(theme::secondary)
                            .padding(Padding {
                                top: 7.0,
                                right: 16.0,
                                bottom: 7.0,
                                left: 16.0,
                            }),
                    )
                    .push(
                        Button::new(Text::new("Save").size(13))
                            .on_press(Message::Save)
                            .style(theme::primary)
                            .padding(Padding {
                                top: 7.0,
                                right: 18.0,
                                bottom: 7.0,
                                left: 18.0,
                            }),
                    ),
            )
            .into()
    }
}

// ── Shared building blocks ──────────────────────────────────────────

/// Pane title + one-paragraph explainer, replacing the old pattern of
/// a bare `Text::new(title).size(24)` followed by unstyled body text.
fn pane_header(
    b: &'static theme::BrandPalette,
    title: &'static str,
    subtitle: String,
) -> Element<'static, Message> {
    Column::new()
        .spacing(6)
        .push(Text::new(title).size(22).font(FONT_BOLD).color(b.ink))
        .push(Text::new(subtitle).size(13).color(b.muted))
        .into()
}

/// Surface card with a hairline border — the pane's main grouping
/// device, mirroring the landing page's feature cards.
fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Container::new(content)
        .style(theme::card)
        .padding(16)
        .width(Length::Fill)
        .into()
}

/// Bold in-card section heading ("Behaviour", "Folders", …).
fn section_title(b: &'static theme::BrandPalette, text: &'static str) -> Element<'static, Message> {
    Text::new(text).size(14).font(FONT_BOLD).color(b.ink).into()
}

/// Muted footnote at the bottom of a pane.
fn tip(b: &'static theme::BrandPalette, text: &'static str) -> Element<'static, Message> {
    Text::new(text).size(11).color(b.muted).into()
}

/// One mono glyph on a raised key — the site's `.keycap`.
fn keycap_chip(text: String) -> Element<'static, Message> {
    Container::new(Text::new(text).size(11).font(Font::MONOSPACE))
        .style(theme::keycap)
        .padding(Padding {
            top: 3.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .into()
}

/// A hotkey combo as a row of keycap chips: `Ctrl+Shift+Space` →
/// [Ctrl] + [Shift] + [Space] — the same rendering the site uses for
/// hotkey chords.
fn hotkey_chips(b: &'static theme::BrandPalette, combo: &str) -> Element<'static, Message> {
    let mut row = Row::new().spacing(4).align_y(Alignment::Center);
    for (i, part) in combo.split('+').enumerate() {
        if i > 0 {
            row = row.push(Text::new("+").size(11).color(b.muted));
        }
        row = row.push(keycap_chip(part.to_owned()));
    }
    row.into()
}

/// Brand-coloured inline link opening `url` in the browser.
fn link_button(label: &'static str, url: &'static str) -> Element<'static, Message> {
    Button::new(Text::new(label).size(13))
        .on_press(Message::OpenUrl(url))
        .style(theme::link)
        .padding(Padding {
            top: 4.0,
            right: 6.0,
            bottom: 4.0,
            left: 6.0,
        })
        .into()
}

/// Quiet bordered button for the Folders row.
fn folder_button(label: &'static str, msg: Message) -> Element<'static, Message> {
    Button::new(Text::new(label).size(12))
        .on_press(msg)
        .style(theme::secondary)
        .padding(Padding {
            top: 5.0,
            right: 12.0,
            bottom: 5.0,
            left: 12.0,
        })
        .into()
}

/// Per-pane status banner text: ecto green for OK, garble pink for
/// errors — the site's fixed/garbled word colours.
fn status_line(b: &'static theme::BrandPalette, banner: &SaveBanner) -> Element<'static, Message> {
    Text::new(banner.text.clone())
        .size(11)
        .color(if banner.is_error { b.garble } else { b.ecto })
        .into()
}
