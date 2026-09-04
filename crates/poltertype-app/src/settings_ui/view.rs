//! Widget tree construction: the `view` half of the iced loop.
//!
//! The visual language mirrors poltertype.com — surface-coloured
//! sidebar, hairline cards, keycap chips, brand-indigo primary actions.
//! All colours come from [`theme::BrandPalette`](super::theme::BrandPalette)
//! via `self.brand()`, so every pane re-themes with the window.

use iced::widget::{
    Button, Checkbox, Column, Container, PickList, Row, Scrollable, Space, Text, TextInput,
    container, rule, text_editor,
};
use iced::{Alignment, Element, Font, Length, Padding};

use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::settings::TrayIconStyle;

use crate::consts::{
    DEFAULT_PAUSE_TOGGLE, DEFAULT_SWITCH_LAST, MACOS_SAFE_PAUSE_TOGGLE, WAYLAND_SAFE_SWITCH_LAST,
};

use super::consts::*;
use super::enums::*;
use super::helpers::*;
use super::state::*;
use super::theme::{self, font_bold};

impl SettingsApp {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let body = match self.pane {
            Pane::Setup => self.view_setup(),
            Pane::Languages => self.view_languages(),
            Pane::Hotkeys => self.view_hotkeys(),
            Pane::Commands => self.view_commands(),
            Pane::Wordlists => self.view_wordlists(),
            Pane::General => self.view_general(),
            Pane::Exceptions => self.view_exceptions(),
            Pane::Suggestions => self.view_suggestions(),
            Pane::Plugins => self.view_plugins(),
            Pane::About => self.view_about(),
        };

        // The Plug-ins pane scrolls its own halves — a plug-in's
        // section list has to stay put while its settings scroll, and
        // a scrollable inside a scrollable cannot do that.
        let scrolls_itself = self.pane == Pane::Plugins && self.plugins.len() == 1;
        let padded = Container::new(body).padding(Padding {
            top: 22.0,
            right: 24.0,
            bottom: 16.0,
            left: 24.0,
        });
        let scrolled: Element<'_, Message> = if scrolls_itself {
            padded.height(Length::Fill).width(Length::Fill).into()
        } else {
            Scrollable::new(padded)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        };
        let content = Column::new().push(scrolled).push(self.view_footer());

        let main = Row::new()
            .push(self.nav_panel())
            .push(rule::vertical(1).style(theme::hairline))
            .push(
                Container::new(content)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .height(Length::Fill);

        // Root backdrop quad. NOT cosmetic: the per-rebuild epsilon in
        // its colour defeats buggy partial presents in iced 0.13's
        // tiny-skia compositor. See [`SettingsApp::backdrop_color`].
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
            .push(theme::mark(32))
            .push(
                Column::new()
                    .push(
                        Text::new("PolterType")
                            .size(16)
                            .font(font_bold())
                            .color(b.ink),
                    )
                    .push(
                        Text::new(tr("ui.settings", "Settings"))
                            .size(11)
                            .color(b.muted),
                    ),
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
                .push(item(
                    setup_nav_label(self.setup.needs_attention()),
                    Pane::Setup,
                ))
                .push(item(tr("nav.languages", "Languages"), Pane::Languages))
                .push(item(tr("nav.hotkeys", "Hotkeys"), Pane::Hotkeys))
                .push(item(tr("nav.commands", "Commands"), Pane::Commands))
                .push(item(tr("nav.wordlists", "Wordlists"), Pane::Wordlists))
                .push(item(tr("nav.general", "General"), Pane::General))
                .push(item(tr("nav.exceptions", "Exceptions"), Pane::Exceptions))
                .push(item(
                    tr("nav.suggestions", "Suggestions"),
                    Pane::Suggestions,
                ))
                .push(item(tr("nav.plugins", "Plug-ins"), Pane::Plugins))
                .push(item(tr("nav.about", "About"), Pane::About))
                .push(Space::new().height(Length::Fill))
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
        // The effective state, not the raw list: an empty allow-list
        // means every OS layout is active, and rendering it literally
        // showed a fresh install zero ticked boxes while every layout
        // was in fact being considered.
        let allow_list = &self.settings.languages.active;
        let implicit_all = allow_list.is_empty();

        // Lead with WHERE the list comes from — first-run users read
        // "en-US / uk-UA" as a product default and hunt for an "add
        // language" button that deliberately does not exist: the OS
        // keyboard configuration is the list, so there is no second one
        // to drift out of sync.
        let subtitle = tr(
            "languages.subtitle",
            "This list mirrors the keyboard layouts enabled in your \
             operating system. To add or remove a language, change your \
             system's keyboard settings, then reopen this window.",
        )
        .to_owned();

        let status = if implicit_all {
            tr(
                "languages.status_all",
                "All of them are currently considered. Untick 'Active' to \
                 restrict PolterType to a subset.",
            )
            .to_owned()
        } else {
            tr_args(
                "languages.status_restricted",
                "Restricted to {} layout(s). Tick more to include them, \
                 or hit 'Reset to defaults' on the About pane to go back \
                 to 'use every OS layout'.",
                &[&allow_list.len().to_string()],
            )
        };

        let mut col = Column::new()
            .spacing(14)
            .push(pane_header(
                b,
                tr("languages.languages", "Languages"),
                subtitle,
            ))
            .push(Text::new(status).size(12).color(b.muted));

        if self.os_layouts.is_empty() {
            col = col.push(card(
                Text::new(tr(
                    "ui.no_os_layouts",
                    "No OS layouts detected. Add languages in your system's keyboard \
                     settings, then reopen this window.",
                ))
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
                            Checkbox::new(is_active_effective)
                                .label(tr("languages.active", "Active"))
                                .text_size(13)
                                .on_toggle({
                                    let id = id.clone();
                                    move |flag| Message::LanguageToggled(id.clone(), flag)
                                })
                                .width(Length::FillPortion(1)),
                        )
                        .push(
                            Checkbox::new(is_ignored)
                                .label(tr("languages.ignore", "Ignore"))
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
            tr(
                "languages.tip",
                "Tip: 'Active' is the allow-list — when nothing is restricted \
                 every OS layout is included. 'Ignore' is a hard veto and \
                 always wins.",
            ),
        ))
        .into()
    }

    pub(super) fn view_hotkeys(&self) -> Element<'_, Message> {
        let b = self.brand();
        // Probed, because this window is a separate process with no
        // listener and no layout switcher. The tray answers the same
        // question off its live backends; showing the configured chord
        // while the tray listened for another one is issue #31.
        let env = poltertype_input::hotkey_environment();
        let row = |label: &'static str, current: &str, kind: HotkeyKind| -> Element<'_, Message> {
            let effective = match kind {
                HotkeyKind::Pause => crate::hotkeys::effective_pause_toggle(current, env),
                HotkeyKind::SwitchLast => crate::hotkeys::effective_switch_last(current, env),
            };
            let current = effective.chord;
            let capturing = self.capturing == Some(kind);
            let display: Element<'_, Message> = if capturing {
                // A single modifier has landed and is waiting for its
                // twin. Without saying so, the pane looks like it
                // ignored the tap — which is what a modifier-only
                // binding's first half is bound to look like.
                let prompt = match self.mod_capture.pending_tap {
                    Some(m) => tr_args(
                        "hotkeys.tap_again",
                        "Tap {} again to bind it…",
                        &[&format_mod_chord(m, false)],
                    ),
                    None => tr(
                        "hotkeys.press_combination_esc_cancel",
                        "Press a combination… (Esc to cancel)",
                    )
                    .to_owned(),
                };
                Text::new(prompt).size(13).color(b.warn).into()
            } else {
                hotkey_chips(b, current)
            };
            let action = if capturing {
                Button::new(Text::new(tr("hotkeys.cancel", "Cancel")).size(12))
                    .on_press(Message::HotkeyRebindCancel)
            } else {
                Button::new(Text::new(tr("hotkeys.rebind", "Rebind")).size(12))
                    .on_press(Message::HotkeyRebindStart(kind))
            };
            let line = Row::new()
                .spacing(16)
                .align_y(Alignment::Center)
                .push(Text::new(label).size(13).width(Length::FillPortion(2)))
                .push(Container::new(display).width(Length::FillPortion(3)))
                .push(action.style(theme::secondary).padding(Padding {
                    top: 5.0,
                    right: 12.0,
                    bottom: 5.0,
                    left: 12.0,
                }));
            let note = match effective.substitution {
                Some(s) => Some(substitution_note(s)),
                // A Caps Lock binding works, and it latches the lock on
                // every press unless the key has been taken out of the
                // layout — which the pane has to say, because the
                // symptom is the corrected word coming back in
                // capitals and nothing about it points here.
                None if current.eq_ignore_ascii_case("capslock") => Some(
                    tr(
                        "hotkeys.caps_lock_still_locks",
                        "PolterType watches this key, it never swallows it, so Caps Lock still \
                         latches — and a latched lock makes the corrected word come back in \
                         capitals. Take the lock off the key first: the `caps:none` keyboard \
                         option, or whatever your remapper calls it.",
                    )
                    .to_owned(),
                ),
                None => None,
            };
            match note {
                None => line.into(),
                // Under the row, not beside it: a sentence does not fit
                // in a table cell.
                Some(n) => Column::new().spacing(4).push(line).push(tip(b, n)).into(),
            }
        };

        Column::new()
            .spacing(14)
            .push(pane_header(
                b,
                tr("hotkeys.hotkeys", "Hotkeys"),
                // Not "registered with the OS": on the Wayland/evdev
                // backend they are read off the key stream instead, and
                // the pane said otherwise while doing exactly that.
                tr(
                    "hotkeys.subtitle",
                    "Hotkeys are global — they fire whatever window has focus. \
                     Click 'Rebind', press the new combination, then save. \
                     The new binding is in force as soon as this window closes.",
                )
                .to_owned(),
            ))
            .push(card(
                Column::new()
                    .spacing(12)
                    .push(row(
                        tr("hotkeys.pause_toggle", "Pause / resume auto-switch"),
                        &self.settings.hotkeys.pause_toggle,
                        HotkeyKind::Pause,
                    ))
                    .push(row(
                        tr("hotkeys.switch_last", "Force-switch the last word"),
                        &self.settings.hotkeys.manual_switch_last,
                        HotkeyKind::SwitchLast,
                    ))
                    .push(self.selection_row(b)),
            ))
            .push(tip(
                b,
                tr_args(
                    "hotkeys.tip",
                    "Tip: a combination needs at least one of {} — a bare \
                     key would clash with typing, and Caps Lock is the one \
                     exception. Modifiers on their own count too: hold two \
                     together ({}), or tap one twice ({}). Those fire when \
                     the keys come back up, and only if nothing else was \
                     pressed in between, so they leave the shortcuts they \
                     are part of alone. \
                     Esc cancels capture without changing anything.",
                    &[
                        &key_list(&["Ctrl", "Alt", "Shift", "Cmd"], " / "),
                        &key_list(&["Ctrl", "Shift"], "+"),
                        &key_list(&["Shift", "Shift"], "+"),
                    ],
                ),
            ))
            .into()
    }

    /// The selection-conversion toggle, under the hotkey it extends.
    ///
    /// Disabled rather than hidden where the session cannot do it: a
    /// setting that exists on one machine and not another is a support
    /// question, and the sentence under it answers that question once
    /// instead of every time. The check is a real probe of this
    /// session's clipboard, not a list of desktop names — GNOME and
    /// Cinnamon's Wayland sessions offer no way to read the clipboard
    /// without taking focus, and a name would not have told us that.
    fn selection_row(&self, b: &'static theme::BrandPalette) -> Element<'static, Message> {
        let available = self.selection_support.is_ok();
        let mut checkbox = Checkbox::new(self.settings.selection.enabled && available)
            .label(tr(
                "hotkeys.also_convert_selection",
                "Also convert selected text",
            ))
            .text_size(13);
        if available {
            checkbox = checkbox.on_toggle(Message::SelectionEnabledToggled);
        }
        let note = match &self.selection_support {
            Ok(()) => tr(
                "hotkeys.selection_note",
                "With this on, the force-switch key converts whatever you have \
                 selected when there is no just-typed word to fix. It copies the \
                 selection to read it, then puts your clipboard back.",
            )
            .to_owned(),
            Err(why) => tr_args(
                "hotkeys.selection_unavailable",
                "Not available here — {}.",
                &[why],
            ),
        };
        Column::new()
            .spacing(4)
            .push(checkbox)
            .push(tip(b, note))
            .into()
    }

    pub(super) fn view_commands(&self) -> Element<'_, Message> {
        let b = self.brand();
        let mut col = Column::new().spacing(14).push(pane_header(
            b,
            tr("commands.commands", "Commands"),
            tr(
                "commands.subtitle",
                "Type a short token, get a phrase — like classic snippet expanders. \
                 For example: typing the trigger `anrl` + space expands into \
                 `Anatomical Reference List `. The engine watches every word \
                 boundary and fires when the typed token matches. Pause / \
                 switch-last live separately on the Hotkeys pane. New commands \
                 take effect after Save + restart.",
            )
            .to_owned(),
        ));

        // ── Existing commands list ──────────────────────────────────
        if self.settings.commands.is_empty() {
            col = col.push(card(
                Text::new(tr(
                    "commands.no_commands_yet_fill",
                    "No commands yet — fill the form below to add one.",
                ))
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

        let mut form = Column::new().spacing(10).push(section_title(
            b,
            tr("commands.add_new_command", "Add a new command"),
        ));

        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label(tr("commands.field_name", "Name")))
                .push(
                    TextInput::new(
                        tr("commands.name_example", "e.g. Insert email signature"),
                        &self.command_draft_name,
                    )
                    .on_input(Message::CommandDraftNameChanged)
                    .style(theme::input)
                    .size(13)
                    .width(Length::FillPortion(4)),
                ),
        );

        // The buffer resets at every word boundary, so a trigger must
        // be a single token; Add refuses any whitespace.
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label(tr("commands.field_trigger", "Trigger")))
                .push(
                    TextInput::new(
                        tr("commands.trigger_example", "e.g. anrl, ;sig, ((en))"),
                        &self.command_draft_trigger,
                    )
                    .on_input(Message::CommandDraftTriggerChanged)
                    .style(theme::input)
                    .size(13)
                    .width(Length::FillPortion(4)),
                ),
        );

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
                .push(label(tr("commands.field_action", "Action")))
                .push(
                    Row::new()
                        .spacing(6)
                        .push(mk_kind_btn(CommandActionKind::TypeText))
                        .push(mk_kind_btn(CommandActionKind::SwitchLayout))
                        .push(mk_kind_btn(CommandActionKind::OpenPath))
                        .width(Length::FillPortion(4)),
                ),
        );

        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label(match self.command_draft_action_kind {
                    CommandActionKind::TypeText => tr("commands.param_text", "Text"),
                    CommandActionKind::SwitchLayout => tr("commands.param_layout", "Layout id"),
                    CommandActionKind::OpenPath => tr("commands.param_path", "Path / URL"),
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

        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label(tr("commands.field_apps", "Apps (optional)")))
                .push(
                    TextInput::new(
                        tr(
                            "commands.apps_example",
                            "comma-separated, e.g. Code.exe,idea64.exe",
                        ),
                        &self.command_draft_apps,
                    )
                    .on_input(Message::CommandDraftAppsChanged)
                    .on_submit(Message::CommandAdd)
                    .style(theme::input)
                    .size(13)
                    .width(Length::FillPortion(4)),
                ),
        );

        let status: Element<'_, Message> = match &self.command_status {
            Some(banner) => status_line(b, banner),
            None => Space::new().width(Length::Shrink).into(),
        };
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(status)
                .push(Space::new().width(Length::Fill))
                .push(
                    Button::new(Text::new(tr("commands.add_command", "Add command")).size(12))
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
                tr(
                    "commands.tip",
                    "Tips: pick triggers that don't collide with words you actually type — \
                     `the` would expand on every English sentence; `;sig` or `((email))` \
                     are safer. Match is exact and case-sensitive. Leave 'Apps' empty for \
                     a global command, or list `OUTLOOK.EXE,thunderbird.exe` to scope a \
                     command (case-insensitive basename match).",
                ),
            ))
            .into()
    }

    pub(super) fn view_wordlists(&self) -> Element<'_, Message> {
        let b = self.brand();
        let mut col = Column::new().spacing(14).push(pane_header(
            b,
            tr("wordlists.wordlists", "Wordlists"),
            tr(
                "wordlists.subtitle",
                "Add language-specific words to the per-layout dictionary \
                 overlay. Use the Save button below to persist your edits, \
                 or just close the window — either way, the engine's \
                 dictionary set refreshes so new words start counting \
                 toward detection on the next typed word, no tray \
                 restart needed.",
            )
            .to_owned(),
        ));

        if self.os_layouts.is_empty() {
            return col
                .push(card(
                    Text::new(tr(
                        "ui.no_os_layouts",
                        "No OS layouts detected. Add languages in your system's \
                         keyboard settings, then reopen this window.",
                    ))
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
        // Only shown once a profile exists; otherwise the row is a
        // single redundant "Global" button. Profiles are added by hand
        // in `[[wordlists.profiles]]` — there is no UI for that yet.
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
                .push(picker_label(tr("wordlists.picker_profile", "Profile")));
            profile_row =
                profile_row.push(profile_btn("", tr("wordlists.profile_global", "Global")));
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
            .push(picker_label(tr("wordlists.picker_layout", "Layout")));
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
                .push(picker_label(tr("wordlists.picker_list", "List")))
                .push(kind_button(WordlistKind::Extras))
                .push(kind_button(WordlistKind::Stop)),
        );

        col = col.push(pickers);

        // ── Resolved-path hint ──────────────────────────────────────
        if let Some(id) = &self.wordlist_layout {
            let path_label =
                match resolve_overlay_path(&self.wordlist_profile, id, self.wordlist_kind) {
                    Some(p) => p.display().to_string(),
                    None => tr(
                        "wordlists.no_config_dir",
                        "(no config dir resolved on this platform)",
                    )
                    .to_owned(),
                };
            col = col.push(
                Text::new(tr_args("wordlists.file_line", "File: {}", &[&path_label]))
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
                .placeholder(tr(
                    "wordlists.editor_placeholder",
                    "# one word per line — '#' starts a comment\n",
                ))
                .into()
        } else {
            Text::new(tr(
                "wordlists.pick_layout_above_start",
                "Pick a layout above to start editing.",
            ))
            .size(13)
            .color(b.muted)
            .into()
        };
        col = col.push(editor);

        let dirty_marker: Element<'_, Message> = if self.wordlist_dirty {
            // Plain text, no bullet glyph — the default UI font on a
            // clean Linux install may lack it and render tofu.
            Text::new(tr("wordlists.unsaved_changes", "unsaved changes"))
                .size(11)
                .color(b.warn)
                .into()
        } else {
            Space::new().width(Length::Shrink).into()
        };
        let status: Element<'_, Message> = match &self.wordlist_status {
            Some(banner) => status_line(b, banner),
            None => Space::new().width(Length::Shrink).into(),
        };

        // No per-pane Save / Reload: the footer pair covers
        // `config.toml` and the active wordlist edit alike. The dirty
        // marker and status banner stay, so "unsaved changes" and
        // auto-save outcomes are still visible.
        col = col.push(
            Row::new()
                .spacing(8)
                .push(dirty_marker)
                .push(Space::new().width(Length::Fill))
                .push(status),
        );

        col.push(tip(
            b,
            tr(
                "wordlists.tip",
                "Tip: Extras helps detection prefer your jargon, \
                 project nouns or family names. Stop list extends the \
                 1- / 2-letter entries the detector accepts as real \
                 words instead of typos.",
            ),
        ))
        .into()
    }

    pub(super) fn view_exceptions(&self) -> Element<'_, Message> {
        let b = self.brand();
        let col = Column::new().spacing(14).push(pane_header(
            b,
            tr("exceptions.exceptions", "Exceptions"),
            tr(
                "exceptions.subtitle",
                "PolterType skips auto-correction when the foreground app's \
                 executable basename is in this list. Manual switch (the \
                 hotkey on the Hotkeys pane) bypasses the list — devs can \
                 still fix wrong-layout identifiers explicitly inside an IDE.",
            )
            .to_owned(),
        ));

        let mut rows = Column::new().spacing(8);
        if self.settings.exceptions.disabled_apps.is_empty() {
            rows = rows.push(
                Text::new(tr(
                    "exceptions.no_exceptions_poltertype_active",
                    "No exceptions — PolterType is active in every app.",
                ))
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
                        TextInput::new(
                            tr("exceptions.example", "e.g. mygame.exe"),
                            &self.exception_draft,
                        )
                        .on_input(Message::ExceptionDraftChanged)
                        .on_submit(Message::ExceptionAdd)
                        .style(theme::input)
                        .size(13)
                        .width(Length::Fill),
                    )
                    .push(
                        Button::new(Text::new(tr("exceptions.add", "Add")).size(13))
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
                tr(
                    "exceptions.tip",
                    "Match is case-insensitive against the basename — both \
                     `code.exe` and `Code.exe` work.",
                ),
            ))
            .into()
    }

    pub(super) fn view_general(&self) -> Element<'_, Message> {
        let b = self.brand();
        let g = &self.settings.general;
        let e = &self.settings.engine;

        // The same flag the tray's pause item writes, named as the mode
        // it is: the request behind it was for manual-only conversion,
        // from someone who had found the pause and read it as the app
        // being off (issue #51).
        let mut conversion_row = Row::new().spacing(6);
        for (manual, label) in [
            (false, tr("general.conversion_auto", "Automatic")),
            (true, tr("general.conversion_manual", "Manual only")),
        ] {
            conversion_row = conversion_row.push(
                Button::new(Text::new(label).size(12))
                    .on_press(Message::ManualOnlyChosen(manual))
                    .style(theme::chip(g.paused == manual))
                    .padding(Padding {
                        top: 5.0,
                        right: 12.0,
                        bottom: 5.0,
                        left: 12.0,
                    }),
            );
        }

        let behaviour = Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.behaviour", "Behaviour")))
            .push(Text::new(tr("general.conversion", "Conversion")).size(12))
            .push(conversion_row)
            .push(
                Text::new(tr(
                    "general.conversion_hint",
                    "Manual only watches what you type but corrects nothing on its own — \
                     the last word is converted when you press the manual hotkey. Same \
                     switch as the tray's Pause auto-switch, so the tray icon shows it too.",
                ))
                .size(11)
                .color(b.muted),
            )
            .push(
                Checkbox::new(g.autostart)
                    .label(tr(
                        "general.start_automatically_when_i",
                        "Start automatically when I sign in",
                    ))
                    .text_size(13)
                    .on_toggle(Message::AutostartToggled),
            )
            .push(
                Checkbox::new(g.sound_on_correct)
                    .label(tr(
                        "general.play_soft_chime_on",
                        "Play a soft chime on correction",
                    ))
                    .text_size(13)
                    .on_toggle(Message::SoundOnCorrectToggled),
            )
            .push(
                Checkbox::new(g.show_notifications)
                    .label(tr(
                        "general.show_second_system_notification",
                        "Show a 2-second system notification on auto-switch",
                    ))
                    .text_size(13)
                    .on_toggle(Message::ShowNotificationsToggled),
            )
            .push(
                Checkbox::new(e.suppress_in_identifiers)
                    .label(tr(
                        "general.skip_auto_switch_on",
                        "Skip auto-switch on identifiers (foo_bar, snake_case, …)",
                    ))
                    .text_size(13)
                    .on_toggle(Message::SuppressInIdentifiersToggled),
            )
            .push(
                Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(Text::new(tr("general.idle_timeout_ms", "Idle timeout (ms):")).size(13))
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
                        Text::new(tr(
                            "general.buffer_cleared_after_this",
                            "Buffer is cleared after this much keyboard silence.",
                        ))
                        .size(11)
                        .color(b.muted),
                    ),
            );

        // Applies instantly; persisted by the footer Save.
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
        // Takes effect where the tray reads it, so unlike the theme
        // above there is nothing to preview here.
        let tray_choice = TrayIconStyle::from_config(&self.settings.general.tray_icon);
        let mut tray_row = Row::new().spacing(6);
        for (choice, label) in [
            (TrayIconStyle::Color, tr("general.tray_color", "Colour")),
            (TrayIconStyle::Flag, tr("general.tray_flag", "Flag")),
            (TrayIconStyle::Mono, tr("general.tray_mono", "Mono")),
            (TrayIconStyle::Hidden, tr("general.tray_hidden", "Hidden")),
        ] {
            tray_row = tray_row.push(
                Button::new(Text::new(label).size(12))
                    .on_press(Message::TrayIconChoiceChanged(choice))
                    .style(theme::chip(tray_choice == choice))
                    .padding(Padding {
                        top: 5.0,
                        right: 12.0,
                        bottom: 5.0,
                        left: 12.0,
                    }),
            );
        }

        // Offers what is on disk, not what was compiled in: a
        // translation dropped into the config directory — or one a
        // plug-in brought with it — is selectable without anybody
        // editing `config.toml` by hand.
        let mut languages: Vec<(String, String)> = vec![(
            tr("general.language_system", "System").to_owned(),
            "system".to_owned(),
        )];
        languages.extend(
            self.ui_languages
                .iter()
                .map(|(code, name)| (language_label(code, name), code.clone())),
        );
        let current = self.settings.general.ui_language.trim();
        let automatic = current.is_empty()
            || current.eq_ignore_ascii_case("system")
            || current.eq_ignore_ascii_case("auto");
        let chosen_language = languages
            .iter()
            .find(|(_, code)| {
                if automatic {
                    code == "system"
                } else {
                    code.eq_ignore_ascii_case(current)
                }
            })
            .map(|(label, _)| label.clone())
            // A code with no catalog on this machine is still what the
            // file says, so it is shown rather than silently replaced
            // by "System".
            .or_else(|| (!automatic).then(|| current.to_owned()));
        let language_pairs = languages.clone();
        let language_row = PickList::new(
            languages
                .into_iter()
                .map(|(label, _)| label)
                .collect::<Vec<_>>(),
            chosen_language,
            move |picked| {
                let code = language_pairs
                    .iter()
                    .find(|(label, _)| *label == picked)
                    .map(|(_, code)| code.clone())
                    .unwrap_or(picked);
                Message::UiLanguageChanged(code)
            },
        )
        .text_size(12)
        .width(Length::Fixed(260.0));

        let appearance = Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.appearance", "Appearance")))
            .push(Text::new(tr("general.interface_language", "Interface language")).size(12))
            .push(language_row)
            .push(
                Text::new(tr(
                    "general.interface_language_hint",
                    "Applies as you pick it; the tray menu follows once you save. Anything \
                     not translated stays English, and a plug-in can bring a language of \
                     its own.",
                ))
                .size(11)
                .color(b.muted),
            )
            .push(Text::new(tr("general.theme", "Theme")).size(12))
            .push(theme_row)
            .push(
                Text::new(tr(
                    "general.system_follows_os_light",
                    "System follows the OS light/dark preference. Save to persist.",
                ))
                .size(11)
                .color(b.muted),
            )
            .push(Text::new(tr("general.tray_icon", "Tray icon")).size(12))
            .push(tray_row)
            .push(
                Text::new(tr(
                    "general.tray_icon_hint",
                    "Colour gives every layout its own; Flag draws its country, and \
                     falls back to Colour where there is no drawing for one; Mono \
                     keeps one neutral badge. Hidden removes the icon and its menu — \
                     reopen this window by running poltertype --settings.",
                ))
                .size(11)
                .color(b.muted),
            );

        // This card is the app's disclosure surface for the one thing it
        // does that reaches outside the machine, so it names the exact
        // URL rather than saying "checks for updates" and leaving the
        // user to trust us.
        let u = &self.settings.updates;
        // `on_press` only when updates are on: an iced Button with no
        // handler renders disabled, which is exactly the signal wanted
        // — with checking off, the interval means nothing.
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
            .push(Text::new(tr("general.check_every_hours", "Check every (hours):")).size(13))
            .push(step("-1", -1))
            .push(
                Text::new(format!("{:>3}", u.check_interval_hours))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(if u.enabled { b.ink } else { b.muted }),
            )
            .push(step("+1", 1));

        let mut updates = Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.updates", "Updates")))
            .push(
                Checkbox::new(u.enabled)
                    .label(tr(
                        "general.download_new_versions_automatically",
                        "Download new versions automatically, install on restart",
                    ))
                    .text_size(13)
                    .on_toggle(Message::AutoUpdateToggled),
            )
            .push(interval_row)
            .push(
                Text::new(tr(
                    "general.updates_disclosure",
                    "This is the only network connection PolterType makes. It fetches a small \
                     version file from GitHub — no account, no identifier, nothing about you or \
                     what you type. A new version is downloaded, checksum-verified and then left \
                     alone until you quit or click \"Restart to update\" in the tray. Never \
                     installed while you're typing.",
                ))
                .size(11)
                .color(b.muted),
            )
            .push(
                Text::new(poltertype_update::MANIFEST_URL)
                    .size(10)
                    .font(Font::MONOSPACE)
                    .color(b.muted),
            );

        // Beside the switch that turns updating on, because that is
        // where the price is paid — the alternative is finding out
        // after the restart, with the app silently deaf.
        if let Some(note) = poltertype_shell::update_permission_note() {
            updates = updates.push(Text::new(note).size(11).color(b.brand));
        }

        let folders = Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.folders", "Folders")))
            .push(
                Row::new()
                    .spacing(8)
                    .push(folder_button(
                        tr("general.open_config", "Open config.toml"),
                        Message::OpenConfigFile,
                    ))
                    .push(folder_button(
                        tr("general.open_logs", "Logs"),
                        Message::OpenLogsDir,
                    ))
                    .push(folder_button(
                        tr("general.open_wordlists", "User wordlists"),
                        Message::OpenWordlistsDir,
                    ))
                    .push(folder_button(
                        tr("general.open_layouts", "User layouts"),
                        Message::OpenLayoutsDir,
                    )),
            );

        Column::new()
            .spacing(14)
            .push(pane_header(
                b,
                tr("general.general", "General"),
                tr(
                    "general.subtitle",
                    "Behaviour of the tray app and the correction engine.",
                )
                .to_owned(),
            ))
            .push(card(behaviour))
            .push(card(appearance))
            .push(card(updates))
            .push(card(folders))
            .into()
    }

    pub(super) fn view_suggestions(&self) -> Element<'_, Message> {
        let b = self.brand();
        let s = &self.settings.suggestions;

        // `on_press` only while suggestions are on — the same
        // handler-less-Button-renders-disabled signal the Updates card
        // uses for its interval row.
        let step = |label: &'static str, msg: Message| {
            let btn = Button::new(Text::new(label).size(12))
                .style(theme::secondary)
                .padding(Padding {
                    top: 4.0,
                    right: 8.0,
                    bottom: 4.0,
                    left: 8.0,
                });
            if s.enabled { btn.on_press(msg) } else { btn }
        };
        let value_color = if s.enabled { b.ink } else { b.muted };

        let max_row = Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(Text::new(tr("suggestions.max_suggestions", "Max suggestions (1–9):")).size(13))
            .push(step("-1", Message::SuggestionMaxDelta(-1)))
            .push(
                Text::new(format!("{:>2}", s.max_suggestions))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(value_color),
            )
            .push(step("+1", Message::SuggestionMaxDelta(1)))
            .push(
                Text::new(tr(
                    "suggestions.each_entry_applied_with",
                    "Each entry is applied with one digit key, so 9 is the ceiling.",
                ))
                .size(11)
                .color(b.muted),
            );

        let timeout_row = Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(
                Text::new(tr(
                    "suggestions.tooltip_timeout_seconds",
                    "Tooltip timeout (seconds):",
                ))
                .size(13),
            )
            .push(step("-5", Message::SuggestionTimeoutDelta(-5)))
            .push(
                Text::new(format!("{:>3}", s.tooltip_timeout_secs))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(value_color),
            )
            .push(step("+5", Message::SuggestionTimeoutDelta(5)))
            .push(
                Text::new(tr(
                    "suggestions.seconds_tooltip_hides_itself",
                    "3–600 seconds; the tooltip hides itself when the time is up.",
                ))
                .size(11)
                .color(b.muted),
            );

        let tooltip_card = Column::new()
            .spacing(12)
            .push(section_title(b, tr("suggestions.tooltip", "Tooltip")))
            .push(
                Checkbox::new(s.enabled)
                    .label(tr(
                        "suggestions.show_suggestions_mistyped_words",
                        "Show suggestions for mistyped words",
                    ))
                    .text_size(13)
                    .on_toggle(Message::SuggestionsToggled),
            )
            .push(max_row)
            .push(timeout_row);

        // A `TextInput` without `on_input` renders disabled.
        let mut modifiers_input = TextInput::new(
            tr("suggestions.modifiers_example", "e.g. Ctrl+Shift"),
            &s.accept_modifiers,
        )
        .style(theme::input)
        .size(13)
        .width(Length::Fixed(180.0));
        if s.enabled {
            modifiers_input = modifiers_input.on_input(Message::SuggestionModifiersChanged);
        }

        let mut chord_card = Column::new()
            .spacing(12)
            .push(section_title(
                b,
                tr("suggestions.keyboard_accept", "Keyboard accept"),
            ))
            .push(
                Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(
                        Text::new(tr(
                            "suggestions.keyboard_accept_modifiers",
                            "Keyboard accept modifiers:",
                        ))
                        .size(13),
                    )
                    .push(modifiers_input),
            )
            .push(
                Text::new(tr_args(
                    "suggestions.modifiers_hint",
                    "'+'-separated: {} — e.g. Ctrl+Shift. Applied with \
                     digit keys 1–9. Leave empty to disable keyboard accept.",
                    &[&named_key_list(&["Ctrl", "Shift", "Alt", "Meta"], ", ")],
                ))
                .size(11)
                .color(b.muted),
            );
        // Non-empty but chord-disabling input (bare `Shift`, a typo)
        // warns instead of being rejected: the engine treats it as "no
        // chord", so without this the setting looks configured while
        // doing nothing.
        if !s.accept_modifiers.trim().is_empty()
            && !accept_modifiers_enable_keyboard(&s.accept_modifiers)
        {
            chord_card = chord_card.push(
                Text::new(tr_args(
                    "suggestions.modifiers_required",
                    "At least one of {} is required — as written, keyboard \
                     accept is off (clicking a suggestion still works).",
                    &[&named_key_list(&["Ctrl", "Alt", "Meta"], " / ")],
                ))
                .size(11)
                .color(b.warn),
            );
        }

        Column::new()
            .spacing(14)
            .push(pane_header(
                b,
                tr("suggestions.suggestions", "Suggestions"),
                tr(
                    "suggestions.subtitle",
                    "Offer dictionary suggestions in a small tooltip when a typed word \
                     looks misspelled. Clicking a suggestion (or pressing the accept \
                     chord + a digit) replaces the word.",
                )
                .to_owned(),
            ))
            .push(card(tooltip_card))
            .push(card(chord_card))
            .push(tip(
                b,
                tr(
                    "suggestions.tip",
                    "Tip: suggestions come from the same bundled dictionaries the detector \
                     already uses. Everything is computed locally — nothing you type leaves \
                     your machine.",
                ),
            ))
            .into()
    }

    pub(super) fn view_about(&self) -> Element<'_, Message> {
        let b = self.brand();

        let hero = Column::new()
            .spacing(6)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .push(Space::new().height(10))
            .push(theme::mark(64))
            .push(Space::new().height(4))
            .push(
                Text::new("PolterType")
                    .size(24)
                    .font(font_bold())
                    .color(b.ink),
            )
            .push(
                Text::new(tr_args(
                    "about.version",
                    "Version {}",
                    &[env!("CARGO_PKG_VERSION")],
                ))
                .size(12)
                .color(b.muted),
            )
            .push(
                Text::new(tr(
                    "about.cross_platform_automatic_keyboard",
                    "Cross-platform automatic keyboard layout switcher.",
                ))
                .size(13),
            )
            .push(
                Row::new()
                    .spacing(4)
                    .push(link_button("poltertype.com", SITE_URL))
                    .push(link_button("GitHub", REPO_URL))
                    .push(link_button(
                        tr("about.report_an_issue", "Report an issue"),
                        ISSUES_URL,
                    )),
            )
            .push(Space::new().height(6));

        let escape_hatches = Column::new()
            .spacing(12)
            .push(section_title(
                b,
                tr(
                    "about.power_user_escape_hatches",
                    "Power-user escape hatches",
                ),
            ))
            .push(
                Row::new()
                    .spacing(8)
                    .push(
                        Button::new(
                            Text::new(tr("about.reset_defaults", "Reset to defaults")).size(13),
                        )
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
                        Button::new(
                            Text::new(tr("about.reload_from_disk", "Reload from disk")).size(13),
                        )
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
                Text::new(tr_args(
                    "about.config_line",
                    "Config: {}",
                    &[&self.config_path.display().to_string()],
                ))
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
            None => Space::new().width(Length::Shrink).into(),
        };

        Column::new()
            .push(rule::horizontal(1).style(theme::hairline))
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
                    .push(Space::new().width(Length::Fill))
                    .push(
                        Button::new(Text::new(tr("footer.reload", "Reload")).size(13))
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
                        Button::new(Text::new(tr("footer.save", "Save")).size(13))
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

/// Pane title + one-paragraph explainer.
pub(super) fn pane_header(
    b: &'static theme::BrandPalette,
    title: &'static str,
    subtitle: String,
) -> Element<'static, Message> {
    Column::new()
        .spacing(6)
        .push(Text::new(title).size(22).font(font_bold()).color(b.ink))
        .push(Text::new(subtitle).size(13).color(b.muted))
        .into()
}

/// Surface card with a hairline border — the pane's main grouping
/// device, mirroring the landing page's feature cards.
pub(super) fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Container::new(content)
        .style(theme::card)
        .padding(16)
        .width(Length::Fill)
        .into()
}

/// Bold in-card section heading ("Behaviour", "Folders", …).
pub(super) fn section_title(
    b: &'static theme::BrandPalette,
    text: &'static str,
) -> Element<'static, Message> {
    Text::new(text)
        .size(14)
        .font(font_bold())
        .color(b.ink)
        .into()
}

/// Sidebar label for the Setup pane. Carries the warning glyph only
/// while something is actually unresolved — a permanent ⚠ in the nav
/// is a warning nobody reads by the second day.
/// How one language is named in the picker. A locale nobody has a name
/// for is offered under its bare code — the file works, so hiding it
/// would be the worse answer.
fn language_label(code: &str, name: &str) -> String {
    if name == code {
        code.to_owned()
    } else {
        format!("{name} ({code})")
    }
}

fn setup_nav_label(needs_attention: bool) -> &'static str {
    // ASCII on purpose: the bundled font has no ⚠ and draws a tofu
    // box, which reads as a rendering bug rather than as a warning.
    if needs_attention {
        tr("nav.setup_attention", "Setup  (!)")
    } else {
        tr("nav.setup", "Setup")
    }
}

/// Why the chord on show is not the one in `config.toml`.
///
/// Named chords rather than "the default": the user cannot see the
/// value that was replaced, and a substitution they cannot name is
/// indistinguishable from a bug — which is how it was reported.
fn substitution_note(s: crate::hotkeys::Substitution) -> String {
    match s {
        crate::hotkeys::Substitution::DefaultIsDestructiveHere => tr_args(
            "hotkeys.substituted_observed_not_consumed",
            "This session reads hotkeys off the key stream, so {} would reach the app you are \
             typing in as well — where it deletes the very word being fixed. {} is used here \
             instead. Rebind to choose your own.",
            &[DEFAULT_SWITCH_LAST, WAYLAND_SAFE_SWITCH_LAST],
        ),
        crate::hotkeys::Substitution::SystemOwnsDefault => tr_args(
            "hotkeys.substituted_system_owns",
            "This system already uses {} to switch input sources, so {} is used here instead. \
             Rebind to choose your own.",
            &[DEFAULT_PAUSE_TOGGLE, MACOS_SAFE_PAUSE_TOGGLE],
        ),
    }
}

/// Muted footnote at the bottom of a pane.
pub(super) fn tip(
    b: &'static theme::BrandPalette,
    text: impl Into<String>,
) -> Element<'static, Message> {
    Text::new(text.into()).size(11).color(b.muted).into()
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

/// A hotkey combo as a row of keycap chips, the same rendering the site
/// uses for chords. On macOS the chips carry the platform glyphs
/// (⌃⇧⌘) via `display_key_token`.
fn hotkey_chips(b: &'static theme::BrandPalette, combo: &str) -> Element<'static, Message> {
    let mut row = Row::new().spacing(4).align_y(Alignment::Center);
    for (i, part) in combo.split('+').enumerate() {
        if i > 0 {
            row = row.push(Text::new("+").size(11).color(b.muted));
        }
        row = row.push(keycap_chip(display_key_token(part)));
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
pub(super) fn status_line(
    b: &'static theme::BrandPalette,
    banner: &SaveBanner,
) -> Element<'static, Message> {
    Text::new(banner.text.clone())
        .size(11)
        .color(if banner.is_error { b.garble } else { b.ecto })
        .into()
}
