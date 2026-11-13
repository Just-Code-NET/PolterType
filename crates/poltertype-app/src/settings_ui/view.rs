//! Widget tree construction: the `view` half of the iced loop.
//!
//! The visual language mirrors poltertype.com — surface-coloured
//! sidebar, hairline cards, keycap chips, brand-indigo primary actions.
//! All colours come from [`theme::BrandPalette`](super::theme::BrandPalette)
//! via `self.brand()`, so every pane re-themes with the window.
//!
//! This file holds the window's frame and the smaller panes; the larger
//! ones have a `view_*` file each, and the pieces they share live in
//! [`widgets`](super::widgets).

use iced::widget::{
    Button, Checkbox, Column, Container, PickList, Row, Scrollable, Space, Text, TextInput,
    container, rule, text, text_editor,
};
use iced::{Alignment, Element, Font, Length, Padding};

use poltertype_core::i18n::{tr, tr_args};

use crate::consts::{
    DEFAULT_PAUSE_TOGGLE, DEFAULT_SWITCH_LAST, MACOS_SAFE_PAUSE_TOGGLE, WAYLAND_SAFE_SWITCH_LAST,
};

use super::consts::*;
use super::enums::*;
use super::helpers::*;
use super::state::*;
use super::theme::{self, font_bold};
use super::widgets::{
    action, button_label, button_run, card, chip, pane_header, section_title, status_line, tip,
};

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
        let padded = Container::new(body).width(Length::Fill).padding(Padding {
            top: 22.0,
            right: 24.0,
            bottom: 16.0,
            left: 24.0,
        });
        let scrolled: Element<'_, Message> = if scrolls_itself {
            padded.height(Length::Fill).into()
        } else {
            Scrollable::new(padded)
                .id(iced::widget::Id::new(PANE_SCROLL))
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        };
        let content = Container::new(Column::new().push(scrolled).push(self.view_footer()))
            .width(Length::Fill)
            .height(Length::Fill);

        let main: Element<'_, Message> = if self.is_narrow() {
            Column::new()
                .push(self.nav_bar())
                .push(rule::horizontal(1).style(theme::hairline))
                .push(content)
                .height(Length::Fill)
                .into()
        } else {
            Row::new()
                .push(self.nav_panel())
                .push(rule::vertical(1).style(theme::hairline))
                .push(content)
                .height(Length::Fill)
                .into()
        };

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

    /// The panes in the order the sidebar lists them, grouped: what the
    /// app does, what extends it, and what is about the app itself.
    ///
    /// Setup waits at the bottom once there is nothing to do there and
    /// moves to the very top while something is unresolved: at that
    /// moment nothing else in this window works anyway.
    fn nav_groups(&self) -> Vec<Vec<(&'static str, Pane)>> {
        let attention = self.setup.needs_attention();
        let setup = (setup_nav_label(attention), Pane::Setup);
        let mut groups = vec![
            vec![
                (tr("nav.general", "General"), Pane::General),
                (tr("nav.languages", "Languages"), Pane::Languages),
                (tr("nav.hotkeys", "Hotkeys"), Pane::Hotkeys),
                (tr("nav.suggestions", "Suggestions"), Pane::Suggestions),
                (tr("nav.commands", "Commands"), Pane::Commands),
                (tr("nav.wordlists", "Wordlists"), Pane::Wordlists),
                (tr("nav.exceptions", "Exceptions"), Pane::Exceptions),
            ],
            vec![(tr("nav.plugins", "Plug-ins"), Pane::Plugins)],
            vec![(tr("nav.about", "About"), Pane::About)],
        ];
        // Wordlists edits the detector's own dictionaries — nobody's
        // first stop. It stays listed while it is the open pane, so
        // switching advanced off never strands someone mid-edit.
        if !self.settings.general.advanced_settings && self.pane != Pane::Wordlists {
            groups[0].retain(|(_, pane)| *pane != Pane::Wordlists);
        }
        if attention {
            groups.insert(0, vec![setup]);
        } else {
            groups[2].insert(0, setup);
        }
        groups
    }

    /// Mark and wordmark, shared by the sidebar and the narrow top bar.
    fn brand_row(&self) -> Row<'_, Message> {
        let b = self.brand();
        Row::new()
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
            )
    }

    /// Branded side navigation: mark + wordmark on top, pane list,
    /// version pinned to the bottom.
    fn nav_panel(&self) -> Element<'_, Message> {
        let b = self.brand();
        let mut list = Column::new().spacing(2);
        for (index, group) in self.nav_groups().into_iter().enumerate() {
            if index > 0 {
                list = list.push(Space::new().height(10));
            }
            for (label, pane) in group {
                list = list.push(
                    Button::new(button_label(label, 13))
                        .on_press(Message::SelectPane(pane))
                        .style(theme::nav(self.pane == pane))
                        .width(Length::Fill)
                        .padding(Padding {
                            top: 7.0,
                            right: 12.0,
                            bottom: 7.0,
                            left: 12.0,
                        }),
                );
            }
        }

        Container::new(
            Column::new()
                .padding(14)
                .push(Container::new(self.brand_row()).padding(Padding {
                    top: 2.0,
                    right: 0.0,
                    bottom: 16.0,
                    left: 2.0,
                }))
                .push(
                    // A window too short for the whole list must still
                    // reach every pane. No visible bar: the content beside
                    // it owns the window's one scrollbar.
                    Scrollable::new(list)
                        .direction(iced::widget::scrollable::Direction::Vertical(
                            iced::widget::scrollable::Scrollbar::new()
                                .width(0)
                                .scroller_width(0),
                        ))
                        .height(Length::Fill),
                )
                .push(Space::new().height(8))
                .push(rule::horizontal(1).style(theme::hairline))
                .push(Space::new().height(8))
                .push(self.advanced_switch())
                .push(Space::new().height(8))
                .push(
                    Text::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .size(11)
                        .color(b.muted),
                ),
        )
        .width(NAV_WIDTH)
        .height(Length::Fill)
        .style(theme::sidebar)
        .into()
    }

    /// The window's one basic/advanced switch. Every pane and every
    /// plug-in reads it, so it lives with the navigation rather than on
    /// any one page.
    fn advanced_switch(&self) -> Element<'_, Message> {
        Checkbox::new(self.settings.general.advanced_settings)
            .label(tr("nav.advanced", "Expert"))
            .text_size(12)
            .size(14)
            .on_toggle(Message::AdvancedToggled)
            .into()
    }

    /// The sidebar's narrow-window form: one drop-down instead of a
    /// column that would leave the panes no width at all.
    fn nav_bar(&self) -> Element<'_, Message> {
        let panes: Vec<(&'static str, Pane)> = self.nav_groups().into_iter().flatten().collect();
        let labels: Vec<&'static str> = panes.iter().map(|(label, _)| *label).collect();
        let selected = panes
            .iter()
            .find(|(_, pane)| *pane == self.pane)
            .map(|(label, _)| *label);
        let picker = PickList::new(labels, selected, move |picked: &'static str| {
            let pane = panes
                .iter()
                .find(|(label, _)| *label == picked)
                .map_or(Pane::General, |(_, pane)| *pane);
            Message::SelectPane(pane)
        })
        .text_size(13)
        .style(theme::pick_list)
        .width(Length::Fill);

        Container::new(
            Row::new()
                .spacing(14)
                .align_y(Alignment::Center)
                .push(self.brand_row())
                .push(picker)
                .push(self.advanced_switch()),
        )
        .padding(Padding {
            top: 10.0,
            right: 16.0,
            bottom: 10.0,
            left: 16.0,
        })
        .width(Length::Fill)
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
                        .spacing(20)
                        .align_y(Alignment::Center)
                        .push(
                            Text::new(id.as_str().to_string())
                                .size(13)
                                .font(Font::MONOSPACE)
                                .width(Length::Fill),
                        )
                        .push(
                            Checkbox::new(is_active_effective)
                                .label(tr("languages.active", "Active"))
                                .text_size(13)
                                .on_toggle({
                                    let id = id.clone();
                                    move |flag| Message::LanguageToggled(id.clone(), flag)
                                }),
                        )
                        .push(
                            Checkbox::new(is_ignored)
                                .label(tr("languages.ignore", "Ignore"))
                                .text_size(13)
                                .on_toggle({
                                    let id = id.clone();
                                    move |flag| Message::LanguageIgnoreToggled(id.clone(), flag)
                                }),
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
            let action_button = if capturing {
                action(
                    tr("hotkeys.cancel", "Cancel"),
                    Some(Message::HotkeyRebindCancel),
                )
            } else {
                action(
                    tr("hotkeys.rebind", "Rebind"),
                    Some(Message::HotkeyRebindStart(kind)),
                )
            };
            let line: Element<'_, Message> = if self.stacked() {
                Column::new()
                    .spacing(6)
                    .push(Text::new(label).size(13))
                    .push(
                        Row::new()
                            .spacing(12)
                            .align_y(Alignment::Center)
                            .push(Container::new(display).width(Length::Fill))
                            .push(action_button),
                    )
                    .into()
            } else {
                Row::new()
                    .spacing(16)
                    .align_y(Alignment::Center)
                    .push(Text::new(label).size(13).width(Length::FillPortion(2)))
                    .push(Container::new(display).width(Length::FillPortion(3)))
                    .push(action_button)
                    .into()
            };
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
                None => line,
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
        // ── "Add new command" form ──────────────────────────────────
        let stacked = self.stacked();
        let line = move |text: &'static str,
                         widget: Element<'static, Message>|
              -> Element<'static, Message> {
            let name = Text::new(text).size(12).color(b.muted);
            if stacked {
                Column::new().spacing(4).push(name).push(widget).into()
            } else {
                Row::new()
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(name.width(Length::Fixed(FORM_LABEL)))
                    .push(widget)
                    .into()
            }
        };

        let mut form = Column::new().spacing(10).push(section_title(
            b,
            tr("commands.add_new_command", "Add a new command"),
        ));

        form = form.push(line(
            tr("commands.field_name", "Name"),
            TextInput::new(
                tr("commands.name_example", "e.g. Insert email signature"),
                &self.command_draft_name,
            )
            .on_input(Message::CommandDraftNameChanged)
            .style(theme::input)
            .size(13)
            .width(Length::Fill)
            .into(),
        ));

        // The buffer resets at every word boundary, so a trigger must
        // be a single token; Add refuses any whitespace.
        form = form.push(line(
            tr("commands.field_trigger", "Trigger"),
            TextInput::new(
                tr("commands.trigger_example", "e.g. anrl, ;sig, ((en))"),
                &self.command_draft_trigger,
            )
            .on_input(Message::CommandDraftTriggerChanged)
            .style(theme::input)
            .size(13)
            .width(Length::Fill)
            .into(),
        ));

        form = form.push(line(
            tr("commands.field_action", "Action"),
            button_run(
                [
                    CommandActionKind::TypeText,
                    CommandActionKind::SwitchLayout,
                    CommandActionKind::OpenPath,
                ]
                .into_iter()
                .map(|kind| {
                    chip(
                        kind.label(),
                        self.command_draft_action_kind == kind,
                        Some(Message::CommandDraftActionKindChanged(kind)),
                    )
                })
                .collect(),
            ),
        ));

        form = form.push(line(
            match self.command_draft_action_kind {
                CommandActionKind::TypeText => tr("commands.param_text", "Text"),
                CommandActionKind::SwitchLayout => tr("commands.param_layout", "Layout id"),
                CommandActionKind::OpenPath => tr("commands.param_path", "Path / URL"),
            },
            TextInput::new(
                self.command_draft_action_kind.placeholder(),
                &self.command_draft_param,
            )
            .on_input(Message::CommandDraftParamChanged)
            .style(theme::input)
            .size(13)
            .width(Length::Fill)
            .into(),
        ));

        form = form.push(line(
            tr("commands.field_apps", "Apps (optional)"),
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
            .width(Length::Fill)
            .into(),
        ));

        let status: Element<'_, Message> = match &self.command_status {
            Some(banner) => status_line(b, banner),
            None => Space::new().width(Length::Shrink).into(),
        };
        form = form.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(Container::new(status).width(Length::Fill))
                .push(
                    action(
                        tr("commands.add_command", "Add command"),
                        Some(Message::CommandAdd),
                    )
                    .style(theme::primary),
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

        // A word list or a row of chips, beside the name of what they pick.
        let picker = |name: &'static str, chips: Vec<Element<'static, Message>>| {
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    Text::new(name)
                        .size(12)
                        .color(b.muted)
                        .width(Length::Fixed(FORM_LABEL)),
                )
                .push(Container::new(button_run(chips)).width(Length::Fill))
        };

        let mut pickers = Column::new().spacing(8);

        // ── Profile picker (Global + each configured profile) ──────
        // Only shown once a profile exists; otherwise the row is a
        // single redundant "Global" button. Profiles are added by hand
        // in `[[wordlists.profiles]]` — there is no UI for that yet.
        if !self.settings.wordlists.profiles.is_empty() {
            let mut chips = vec![chip(
                tr("wordlists.profile_global", "Global"),
                self.wordlist_profile.is_empty(),
                Some(Message::WordlistProfileSelected(String::new())),
            )];
            for p in &self.settings.wordlists.profiles {
                let label = if p.name.is_empty() {
                    p.id.clone()
                } else {
                    p.name.clone()
                };
                chips.push(chip(
                    label,
                    self.wordlist_profile == p.id,
                    Some(Message::WordlistProfileSelected(p.id.clone())),
                ));
            }
            pickers = pickers.push(picker(tr("wordlists.picker_profile", "Profile"), chips));
        }

        // ── Layout picker (one chip per OS-active layout) ───────────
        pickers = pickers.push(picker(
            tr("wordlists.picker_layout", "Layout"),
            self.os_layouts
                .iter()
                .map(|id| {
                    chip(
                        id.as_str().to_string(),
                        self.wordlist_layout.as_ref() == Some(id),
                        Some(Message::WordlistLayoutSelected(id.clone())),
                    )
                })
                .collect(),
        ));

        // ── Kind picker (Extras vs Stop) ────────────────────────────
        pickers = pickers.push(picker(
            tr("wordlists.picker_list", "List"),
            [WordlistKind::Extras, WordlistKind::Stop]
                .into_iter()
                .map(|kind| {
                    chip(
                        kind.label(),
                        self.wordlist_kind == kind,
                        Some(Message::WordlistKindSelected(kind)),
                    )
                })
                .collect(),
        ));

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
                        action(tr("exceptions.add", "Add"), Some(Message::ExceptionAdd))
                            .style(theme::primary),
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
    pub(super) fn view_footer(&self) -> Element<'_, Message> {
        let b = self.brand();
        // `Fill` rather than the text's natural width: a row measures its
        // shrinking children first and in order, so a long path on the
        // left used to take the width the buttons on its right needed.
        let banner = self.save_banner.as_ref().map(|banner| {
            Text::new(&banner.text)
                .size(12)
                .color(if banner.is_error { b.garble } else { b.ecto })
                .wrapping(text::Wrapping::WordOrGlyph)
                .width(Length::Fill)
        });
        let buttons = Row::new()
            .spacing(10)
            .push(
                Button::new(button_label(tr("footer.reload", "Reload"), 13))
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
                Button::new(button_label(tr("footer.save", "Save"), 13))
                    .on_press(Message::Save)
                    .style(theme::primary)
                    .padding(Padding {
                        top: 7.0,
                        right: 18.0,
                        bottom: 7.0,
                        left: 18.0,
                    }),
            );
        let padding = Padding {
            top: 12.0,
            right: 24.0,
            bottom: 14.0,
            left: 24.0,
        };
        let bar: Element<'_, Message> = match banner {
            // Too little room beside the buttons to read a sentence.
            Some(banner) if self.content_width() < FOOTER_STACK_BELOW => Column::new()
                .spacing(8)
                .padding(padding)
                .push(banner)
                .push(
                    Row::new()
                        .push(Space::new().width(Length::Fill))
                        .push(buttons),
                )
                .into(),
            Some(banner) => Row::new()
                .padding(padding)
                .spacing(14)
                .align_y(Alignment::Center)
                .push(banner)
                .push(buttons)
                .into(),
            None => Row::new()
                .padding(padding)
                .push(Space::new().width(Length::Fill))
                .push(buttons)
                .into(),
        };

        Column::new()
            .push(rule::horizontal(1).style(theme::hairline))
            .push(bar)
            .into()
    }
}

// ── Pieces only these panes use ─────────────────────────────────────

/// Sidebar label for the Setup pane. Carries the warning glyph only
/// while something is actually unresolved — a permanent ⚠ in the nav
/// is a warning nobody reads by the second day.
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
