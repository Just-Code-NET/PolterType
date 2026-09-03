//! The Plug-ins pane: what is installed, and its own settings.
//!
//! Every control is drawn by us from a static declaration. A plug-in
//! never renders anything, which is what stops a third-party pane
//! imitating a system prompt, PolterType's own dialogs, or another
//! plug-in.
//!
//! Two things the pane says out loud: **where a plug-in came from**, so
//! one found in a checkout or on `POLTERTYPE_PLUGIN_PATH` is visibly
//! code nobody installed; and **which file is being edited**, since
//! these settings live in the plug-in's config, not ours.
//!
//! A capable plug-in has a hundred settings, so sections become a
//! second navigation list beside the window's own, one section on
//! screen at a time, every control on the same two-column grid.
//!
//! **Exactly one thing on this pane scrolls.** A room list or a report
//! in its own scrolling box put a second bar a few pixels from the
//! first, and a wheel over the boundary then moves whichever one the
//! pointer happened to be over. So those grow to their content.

use iced::widget::{
    Button, Checkbox, Column, Container, PickList, Row, Scrollable, Space, Text, TextInput, rule,
};
use iced::{Alignment, Element, Length, Padding};
use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::plugins::{ControlKind, PaneControl, SettingValue};

use super::consts::{
    LABEL_GAP, NUMBER_WIDTH, PLUGIN_DEFAULT, PLUGIN_DEFAULT_SHORT, PLUGIN_LIST_HINT, SECTION_NAV,
    SUGGEST_ROWS, VALUE_COLUMN,
};
use super::enums::*;
use super::plugin_pane::{CommandOutput, Slot};
use super::state::*;
use super::theme::{self, FONT_MONO, font_bold};
use super::view::section_title;

impl SettingsApp {
    pub(super) fn view_plugins(&self) -> Element<'_, Message> {
        let b = self.brand();

        if self.plugins.is_empty() {
            return Container::new(
                Column::new()
                    .spacing(10)
                    .push(section_title(b, tr("plugins.plugins", "Plug-ins")))
                    .push(
                        Text::new(tr(
                            "plugins.none_installed",
                            "No plug-ins are installed. A plug-in is a separate program that \
                             PolterType runs and shows here; it is never loaded into PolterType \
                             itself.",
                        ))
                        .size(13)
                        .color(b.muted),
                    ),
            )
            .padding(20)
            .into();
        }

        // No padding and no card of its own: the window already pads
        // every pane, and a frame inside that frame made this page sit
        // further from the edge than every other one.
        let mut body = Column::new()
            .spacing(12)
            .push(section_title(b, tr("plugins.plugins", "Plug-ins")));
        for (index, _) in self.plugins.iter().enumerate() {
            if index > 0 {
                body = body.push(rule::horizontal(1).style(theme::hairline));
            }
            body = body.push(self.plugin_card(index));
        }
        Container::new(body).height(self.pane_height()).into()
    }

    /// The pane fills the window when there is one plug-in to lay out —
    /// that is what lets its section list stay put while its settings
    /// scroll. Several plug-ins stack and scroll together instead.
    fn pane_height(&self) -> Length {
        if self.plugins.len() == 1 {
            Length::Fill
        } else {
            Length::Shrink
        }
    }

    /// One plug-in: who it is, then its settings.
    fn plugin_card(&self, plugin: usize) -> Element<'_, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];

        let mut heading = Row::new().spacing(8).align_y(Alignment::Center).push(
            Text::new(format!("{} {}", pane.ext.name, pane.ext.version))
                .size(15)
                .font(font_bold()),
        );
        if pane.ext.development {
            // Not a badge of honour: this is code that was never
            // installed, found next to a source checkout.
            heading = heading.push(
                Text::new(tr("plugins.development_build", "· development build"))
                    .size(12)
                    .color(b.warn),
            );
        }

        let mut head = Column::new().spacing(4).push(heading);
        if !pane.ext.manifest.summary.is_empty() {
            head = head.push(
                Text::new(&pane.ext.manifest.summary)
                    .size(12)
                    .color(b.muted),
            );
        }
        head = head.push(
            Text::new(tr_args(
                "plugins.settings_file",
                "Settings file: {}",
                &[&pane.config_path.display().to_string()],
            ))
            .size(11)
            .color(b.muted),
        );

        let sections = pane.sections();
        // The page's one scrolling region: the settings column when this
        // pane owns the window's height (so the section list stays put
        // beside it), otherwise the window's own scrollbar.
        let owns_height = self.plugins.len() == 1;
        let controls = Container::new(self.plugin_controls(plugin))
            .padding(Padding {
                top: 0.0,
                right: if owns_height { 12.0 } else { 0.0 },
                bottom: 8.0,
                left: 0.0,
            })
            .width(Length::Fill);
        let settings: Element<'_, Message> = if owns_height {
            Scrollable::new(controls)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        } else {
            controls.into()
        };

        let body: Element<'_, Message> = if sections.is_empty() {
            // A small plug-in does not need navigating.
            settings
        } else {
            Row::new()
                .spacing(16)
                .push(self.section_nav(plugin, &sections))
                .push(settings)
                .height(self.pane_height())
                .into()
        };

        let mut card = Column::new()
            .spacing(10)
            .push(head)
            .push(rule::horizontal(1).style(theme::hairline))
            .push(body);

        if let Some(status) = &pane.status {
            card = card.push(Text::new(status).size(11).color(b.muted));
        }

        Container::new(card)
            .width(Length::Fill)
            .height(self.pane_height())
            .into()
    }

    /// The plug-in's own section list, in the window's nav idiom.
    fn section_nav<'a>(&'a self, plugin: usize, sections: &[usize]) -> Element<'a, Message> {
        let pane = &self.plugins[plugin];
        let mut nav = Column::new().spacing(2).width(Length::Fixed(SECTION_NAV));
        for control in sections {
            let Some(declared) = pane.control(*control) else {
                continue;
            };
            nav = nav.push(
                Button::new(Text::new(declared.label.as_str()).size(12))
                    .style(theme::nav(pane.selected_section() == Some(*control)))
                    .width(Length::Fill)
                    .padding(Padding {
                        top: 6.0,
                        right: 10.0,
                        bottom: 6.0,
                        left: 10.0,
                    })
                    .on_press(Message::PluginSectionSelected(plugin, *control)),
            );
        }
        nav.into()
    }

    /// Every control on show: the ones above the first section, then
    /// the selected section's own.
    fn plugin_controls(&self, plugin: usize) -> Column<'_, Message> {
        let pane = &self.plugins[plugin];
        let mut column = Column::new().spacing(13);
        // Buttons declared next to each other are one row of actions:
        // stacked, three of them cost a third of the page and read as
        // three unrelated decisions.
        let mut actions: Option<Row<'_, Message>> = None;
        for (index, control) in pane.ext.manifest.pane.iter().enumerate() {
            if !pane.is_visible(index) {
                continue;
            }
            if control.kind == ControlKind::Button {
                let row = actions.take().unwrap_or_else(|| Row::new().spacing(8));
                actions = Some(row.push(self.plugin_button(plugin, control)));
                continue;
            }
            if let Some(row) = actions.take() {
                column = column.push(row);
            }
            column = column.push(self.plugin_control(plugin, index, control));
        }
        match actions {
            Some(row) => column.push(row),
            None => column,
        }
    }

    /// One action button.
    fn plugin_button<'a>(
        &'a self,
        plugin: usize,
        control: &'a PaneControl,
    ) -> Element<'a, Message> {
        Button::new(Text::new(control.label.as_str()).size(13))
            .style(theme::primary)
            .padding(Padding {
                top: 7.0,
                right: 14.0,
                bottom: 7.0,
                left: 14.0,
            })
            .on_press(Message::PluginCommandClicked(
                plugin,
                control.command.clone(),
            ))
            .into()
    }

    /// One declared control, rendered natively.
    fn plugin_control<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        // The *stored* value, which may be absent — the plug-in then
        // applies a default we do not know. Showing a fabricated 0 or a
        // blank choice would be the pane lying about the config.
        let stored = pane.values.get(index).and_then(|v| v.clone());
        let value = pane.value_of(index);
        let typed = pane.display_of(index).unwrap_or_default();

        match control.kind {
            // Repeated here rather than only in the nav: the nav says
            // where you are among thirteen, this says what you are
            // looking at, with the sentence explaining it.
            ControlKind::Section => {
                let mut head = Column::new().spacing(6).push(
                    Text::new(control.label.as_str())
                        .size(15)
                        .font(font_bold())
                        .color(b.ink),
                );
                if !control.help.is_empty() {
                    head = head.push(
                        Text::new(control.help.as_str())
                            .size(12)
                            .color(b.muted)
                            .width(Length::Fill),
                    );
                }
                head.push(rule::horizontal(1).style(theme::hairline)).into()
            }

            ControlKind::Toggle => self.field(
                control,
                Checkbox::new(matches!(value, SettingValue::Bool(true)))
                    .label("")
                    .on_toggle(move |on| Message::PluginToggled(plugin, index, on))
                    .into(),
            ),

            // A drop-down suits a handful of words — `ask`, `auto`,
            // `off` — but shows one alternative at a time and has
            // nowhere to put a sentence about each. Described options
            // are drawn as a column instead, so they can be compared.
            ControlKind::Choice if control.options.iter().any(|o| o.is_described()) => self
                .plugin_choice_cards(
                    plugin,
                    index,
                    control,
                    stored.as_ref().map(SettingValue::as_display),
                ),

            ControlKind::Choice => self.field(
                control,
                choice_picker(
                    control,
                    stored.as_ref().map(SettingValue::as_display),
                    13,
                    move |value| Message::PluginChoiceSelected(plugin, index, value),
                ),
            ),

            ControlKind::Number | ControlKind::Decimal => self.field(
                control,
                Row::new()
                    .push(Space::new().width(Length::Fill))
                    .push(
                        TextInput::new(tr("plugins.default_short", PLUGIN_DEFAULT_SHORT), &typed)
                            .size(13)
                            .align_x(Alignment::End)
                            .width(Length::Fixed(NUMBER_WIDTH))
                            .on_input(move |text| Message::PluginTextChanged(plugin, index, text)),
                    )
                    .into(),
            ),

            // A box you can type into with the answers offered beside
            // it. Wide, like the text box it is a better version of.
            ControlKind::Suggest => {
                let slot = Slot::control(index);
                let widget =
                    self.plugin_suggest(plugin, slot, pane.display_of(index), move |text| {
                        Message::PluginTextChanged(plugin, index, text)
                    });
                let mut column = self.described(control).spacing(6).push(widget);
                if let Some(note) = self.suggest_note(plugin, slot) {
                    column = column.push(note);
                }
                column.width(Length::Fill).into()
            }

            // Wide by itself: an endpoint URL or a list of host names
            // does not fit in the value column, and a box you have to
            // scroll sideways to read is worse than a wider row.
            ControlKind::Text | ControlKind::Strings => self.wide_field(
                control,
                TextInput::new(
                    if control.kind == ControlKind::Strings {
                        tr("plugins.list_hint", PLUGIN_LIST_HINT)
                    } else {
                        tr("plugins.default", PLUGIN_DEFAULT)
                    },
                    &typed,
                )
                .size(13)
                .width(Length::Fill)
                .on_input(move |text| Message::PluginTextChanged(plugin, index, text))
                .into(),
            ),

            // Reached only when a button is the sole control of its
            // kind in a run; the run itself is laid out by the caller.
            ControlKind::Button => self.plugin_button(plugin, control),

            ControlKind::Report => self.plugin_report(plugin, index, control),

            ControlKind::List => self.plugin_list(plugin, index, control),

            ControlKind::Records => self.plugin_records(plugin, index, control),

            // Said plainly, in place of the control. The alternative —
            // rendering nothing — leaves a plug-in looking like it
            // forgot half its settings.
            ControlKind::Unknown => Text::new(tr_args(
                "plugins.unknown_control",
                "“{}” needs a newer version of PolterType.",
                &[&control.label],
            ))
            .size(12)
            .color(b.warn)
            .into(),
        }
    }

    /// A box you can type into, with what the plug-in knows of listed
    /// under it and narrowing as you type.
    ///
    /// Free text is still free text: the list saves retyping a name
    /// that is on screen elsewhere, it is not a set of permitted
    /// answers — a conversation in a client that is not running has no
    /// row here, and naming it must stay possible.
    ///
    /// **Drawn inline, and bounded.** iced's own combo box puts its
    /// options in an overlay sized to fit them, and ninety-five
    /// conversations covered the whole form. So at most
    /// [`SUGGEST_ROWS`] matches are drawn under the box and the
    /// remainder counted rather than scrolled.
    fn plugin_suggest<'a>(
        &'a self,
        plugin: usize,
        slot: Slot,
        current: Option<String>,
        on_typed: impl Fn(String) -> Message + 'static,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        let has_list = pane.command_id(slot).is_some() || !pane.suggestions(slot).is_empty();

        let mut box_row = Row::new().spacing(6).align_y(Alignment::Center).push(
            TextInput::new(
                tr("plugins.default", PLUGIN_DEFAULT),
                current.as_deref().unwrap_or_default(),
            )
            .size(13)
            .width(Length::Fill)
            .on_input(on_typed),
        );
        if has_list {
            // A word, not an arrow: `↓` is in the bundled Fira Sans and
            // still drew an empty box here, measured on screen — what
            // the renderer resolves `Font::DEFAULT` to is not what the
            // file says.
            box_row = box_row.push(
                Button::new(
                    Text::new(if pane.suggest_open(slot) {
                        tr("plugins.hide", "hide")
                    } else {
                        tr("plugins.list", "list")
                    })
                    .size(11),
                )
                .style(theme::secondary)
                .padding(Padding {
                    top: 4.0,
                    right: 9.0,
                    bottom: 4.0,
                    left: 9.0,
                })
                .on_press(Message::PluginSuggestToggled(plugin, slot)),
            );
        }

        let mut column = Column::new().spacing(4).width(Length::Fill).push(box_row);
        if has_list && pane.suggest_open(slot) {
            column = column.push(self.suggest_list(plugin, slot));
        }
        // Under the box rather than beside the label when the list is
        // closed: this is where the eye already is.
        if let Some(note) = self.suggest_note(plugin, slot) {
            column = column.push(note);
        }
        let _ = b;
        column.into()
    }

    /// The matches themselves: one press each, writing the value.
    fn suggest_list<'a>(&'a self, plugin: usize, slot: Slot) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        let matches = pane.suggestions_matching(slot);
        let mut list = Column::new().spacing(2).width(Length::Fill);

        if matches.is_empty() {
            list = list.push(
                Text::new(match pane.pending(slot) {
                    // Not an error: a name the plug-in has never seen is
                    // exactly what this box exists to still allow.
                    Some(_) => tr(
                        "plugins.nothing_matches",
                        "Nothing here matches — what you typed is used as written.",
                    ),
                    None => tr(
                        "plugins.offered_nothing",
                        "The plug-in offered nothing — type it in.",
                    ),
                })
                .size(11)
                .color(b.muted),
            );
        }
        for (value, detail) in matches.iter().take(SUGGEST_ROWS) {
            let mut label = Column::new()
                .spacing(1)
                .push(Text::new(value.clone()).size(12).color(b.ink));
            if !detail.trim().is_empty() {
                label = label.push(Text::new(detail.clone()).size(10).color(b.muted));
            }
            list = list.push(
                Button::new(label)
                    .style(theme::nav(false))
                    .width(Length::Fill)
                    .padding(Padding {
                        top: 4.0,
                        right: 8.0,
                        bottom: 4.0,
                        left: 8.0,
                    })
                    .on_press(Message::PluginSuggestPicked(plugin, slot, value.clone())),
            );
        }
        if matches.len() > SUGGEST_ROWS {
            list = list.push(
                Text::new(tr_args(
                    "plugins.more_matches",
                    "…and {} more — type to narrow",
                    &[&(matches.len() - SUGGEST_ROWS).to_string()],
                ))
                .size(10)
                .color(b.muted),
            );
        }

        Container::new(list)
            .style(theme::card)
            .padding(6)
            .width(Length::Fill)
            .into()
    }

    /// What a suggestion box has to say for itself under the box.
    ///
    /// Only where it is worth the line. A box inside a card stays quiet
    /// while working — six cards deep, a Refresh button each is six ways
    /// to ask one question — but it does report a failure, because
    /// "nothing offered" and "the client is not running" look identical
    /// in an empty list.
    fn suggest_note<'a>(&'a self, plugin: usize, slot: Slot) -> Option<Element<'a, Message>> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        pane.command_id(slot)?;
        let in_a_card = slot.row.is_some();
        match pane.output(slot) {
            Some(CommandOutput::Failed(why)) => Some(
                Text::new(tr_args(
                    "plugins.could_not_ask",
                    "Could not ask the plug-in: {}",
                    &[why.as_str()],
                ))
                .size(11)
                .color(b.warn)
                .width(Length::Fill)
                .into(),
            ),
            _ if in_a_card => None,
            None | Some(CommandOutput::Loading) => Some(
                Text::new(tr("plugins.asking", "Asking the plug-in…"))
                    .size(11)
                    .color(b.muted)
                    .into(),
            ),
            Some(CommandOutput::Ready(_)) => Some(
                Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(
                        Text::new(match pane.list_rows(slot).len() {
                            0 => tr(
                                "plugins.offered_nothing",
                                "The plug-in offered nothing — type it in.",
                            )
                            .to_owned(),
                            n => tr_args(
                                "plugins.offered_count",
                                "{} offered · type to narrow, or write your own",
                                &[&n.to_string()],
                            ),
                        })
                        .size(11)
                        .color(b.muted),
                    )
                    .push(
                        Button::new(Text::new(tr("plugins.refresh", "Refresh")).size(11))
                            .padding(Padding {
                                top: 3.0,
                                right: 9.0,
                                bottom: 3.0,
                                left: 9.0,
                            })
                            .on_press(Message::PluginOutputRefresh(plugin, slot)),
                    )
                    .into(),
            ),
        }
    }

    /// The short form of [`Self::suggest_note`], for the label line of a
    /// box inside a card.
    ///
    /// A `suggest` renders as a plain text box, so nothing on screen
    /// would otherwise say that ninety-five conversations are one
    /// keystroke away. Six words beside the label say it; a note under
    /// every box in a six-card group would be eighteen lines of the
    /// same sentence.
    fn suggest_hint<'a>(&'a self, plugin: usize, slot: Slot) -> Option<Element<'a, Message>> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        pane.command_id(slot)?;
        let (text, colour) = match pane.output(slot) {
            Some(CommandOutput::Failed(_)) => (
                tr("plugins.hint_failed", "· could not ask the plug-in").to_owned(),
                b.warn,
            ),
            None | Some(CommandOutput::Loading) => {
                (tr("plugins.hint_asking", "· asking…").to_owned(), b.muted)
            }
            Some(CommandOutput::Ready(_)) => (
                match pane.list_rows(slot).len() {
                    0 => tr("plugins.hint_nothing", "· nothing offered — type it in").to_owned(),
                    n => tr_args(
                        "plugins.hint_count",
                        "· {} to pick from, or write your own",
                        &[&n.to_string()],
                    ),
                },
                b.muted,
            ),
        };
        Some(Text::new(text).size(10).color(colour).into())
    }

    /// One row of the form: what the setting is on the left, with its
    /// explanation under it, and what it is set to on the right.
    ///
    /// The explanation belongs on the wide side: under the *control* it
    /// gets 200 pixels, and a paragraph that narrow is a stack of
    /// three-word lines pushing every later row down the page.
    fn field<'a>(
        &'a self,
        control: &'a PaneControl,
        widget: Element<'a, Message>,
    ) -> Element<'a, Message> {
        Row::new()
            .spacing(LABEL_GAP)
            .align_y(Alignment::Center)
            .push(self.described(control).width(Length::Fill))
            .push(
                Container::new(widget)
                    .width(Length::Fixed(VALUE_COLUMN))
                    .align_x(Alignment::End),
            )
            .into()
    }

    /// A setting whose value needs the whole width: the description,
    /// then the box under it.
    fn wide_field<'a>(
        &'a self,
        control: &'a PaneControl,
        widget: Element<'a, Message>,
    ) -> Element<'a, Message> {
        self.described(control)
            .spacing(6)
            .push(widget)
            .width(Length::Fill)
            .into()
    }

    /// A setting's name, and the sentence explaining it.
    fn described<'a>(&'a self, control: &'a PaneControl) -> Column<'a, Message> {
        let b = self.brand();
        let mut column = Column::new()
            .spacing(3)
            .push(Text::new(control.label.as_str()).size(13).color(b.ink));
        if !control.help.is_empty() {
            column = column.push(
                Text::new(control.help.as_str())
                    .size(11)
                    .color(b.muted)
                    .width(Length::Fill),
            );
        }
        column
    }

    /// A repeating group: one card per entry, with the plug-in's
    /// declared fields inside it, plus Add and Remove.
    ///
    /// The card is what makes a list of *composite* things readable.
    /// Laid out as flat rows, six scheduled messages of five fields each
    /// are thirty controls in one column with no way to see where one
    /// message ends; boxed and numbered, they are six things.
    fn plugin_records<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        let rows = pane.record_rows(index);
        let mut column = self.described(control).spacing(10);

        if rows.is_empty() {
            column = column.push(
                Text::new(tr("plugins.nothing_yet", "Nothing here yet."))
                    .size(12)
                    .color(b.muted)
                    .width(Length::Fill),
            );
        }

        let small = Padding {
            top: 3.0,
            right: 8.0,
            bottom: 3.0,
            left: 8.0,
        };
        for row in 0..rows.len() {
            // Remove goes last and apart: it is the one button here that
            // cannot be undone.
            let mut header = Row::new().spacing(6).align_y(Alignment::Center).push(
                Text::new(format!("{}", row + 1))
                    .size(12)
                    .font(font_bold())
                    .color(b.muted)
                    .width(Length::Fill),
            );
            let named = pane.record_id(index, row).is_some();
            let running = pane.action_running(index, row);
            for action in &control.actions {
                // While it runs the button says so and stops being
                // pressable: these actions steal focus, so two at once
                // would type into each other's window. The label changes
                // rather than the button vanishing — a control that
                // disappears under the pointer is worse than one that
                // waits.
                let label = if running {
                    format!("{}…", action.label)
                } else {
                    action.label.clone()
                };
                let mut button = Button::new(Text::new(label).size(12))
                    .style(theme::secondary)
                    .padding(small);
                // A row with nothing in its naming field is one the
                // plug-in has never heard of: the button stays visible
                // and dead, which says "fill this in" where a hidden
                // button would say nothing.
                if named && !pane.any_action_running() {
                    button = button.on_press(Message::PluginRecordAction(
                        plugin,
                        index,
                        row,
                        action.command.clone(),
                    ));
                }
                header = header.push(button);
            }
            let mut card = Column::new().spacing(8).width(Length::Fill).push(
                header.push(
                    Button::new(Text::new(tr("plugins.remove", "Remove")).size(12))
                        .style(theme::danger)
                        .padding(small)
                        .on_press(Message::PluginRecordRemoved(plugin, index, row)),
                ),
            );
            for (position, field) in control.fields.iter().enumerate() {
                card = card.push(self.plugin_record_field(plugin, index, row, position, field));
            }
            column = column.push(
                Container::new(card)
                    .style(theme::card)
                    .padding(12)
                    .width(Length::Fill),
            );
        }

        column
            .push(
                Button::new(
                    Text::new(if control.add_label.trim().is_empty() {
                        tr("plugins.add", "Add")
                    } else {
                        control.add_label.trim()
                    })
                    .size(12),
                )
                .style(theme::secondary)
                .padding(Padding {
                    top: 4.0,
                    right: 10.0,
                    bottom: 4.0,
                    left: 10.0,
                })
                .on_press(Message::PluginRecordAdded(plugin, index)),
            )
            .width(Length::Fill)
            .into()
    }

    /// One field inside one row of a repeating group.
    ///
    /// The same shapes the top-level controls use, addressed by (row,
    /// field) instead of by control index. Label-above-box rather than
    /// the two-column grid: inside a card there is no value column to
    /// line up with, and a field for a whole message wants the width.
    fn plugin_record_field<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        row: usize,
        position: usize,
        field: &'a PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        let key = field.key.clone();
        let typed = pane.record_display(index, row, &field.key);
        let stored = pane.record_value(index, row, &field.key);

        let widget: Element<'a, Message> = match field.kind {
            ControlKind::Suggest => {
                let slot = Slot::field(index, row, position);
                let key = key.clone();
                self.plugin_suggest(plugin, slot, typed.clone(), move |text| {
                    Message::PluginRecordTyped(plugin, index, row, key.clone(), text)
                })
            }

            ControlKind::Toggle => Checkbox::new(matches!(stored, Some(SettingValue::Bool(true))))
                .label(field.label.as_str())
                .text_size(12)
                .on_toggle({
                    let key = key.clone();
                    move |on| {
                        Message::PluginRecordChanged(
                            plugin,
                            index,
                            row,
                            key.clone(),
                            SettingValue::Bool(on),
                        )
                    }
                })
                .into(),

            ControlKind::Choice => {
                choice_picker(field, stored.as_ref().map(SettingValue::as_display), 12, {
                    let key = key.clone();
                    move |value| {
                        Message::PluginRecordChanged(
                            plugin,
                            index,
                            row,
                            key.clone(),
                            SettingValue::Text(value),
                        )
                    }
                })
            }

            _ => TextInput::new(
                tr("plugins.default_short", PLUGIN_DEFAULT_SHORT),
                typed.as_deref().unwrap_or_default(),
            )
            .size(12)
            .width(Length::Fill)
            .on_input({
                let key = key.clone();
                move |text| Message::PluginRecordTyped(plugin, index, row, key.clone(), text)
            })
            .into(),
        };

        // A toggle carries its own label; anything else gets one above.
        let slot = Slot::field(index, row, position);
        let mut column = Column::new().spacing(3).width(Length::Fill);
        if field.kind != ControlKind::Toggle {
            let mut heading = Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(Text::new(field.label.as_str()).size(11).color(b.muted));
            if let Some(hint) = self.suggest_hint(plugin, slot) {
                heading = heading.push(hint);
            }
            column = column.push(heading);
        }
        let mut column = column.push(widget);
        if let Some(note) = self.suggest_note(plugin, slot) {
            column = column.push(note);
        }
        if !field.help.trim().is_empty() {
            column = column.push(
                Text::new(field.help.as_str())
                    .size(10)
                    .color(b.muted)
                    .width(Length::Fill),
            );
        }
        column.into()
    }

    /// A choice whose alternatives need reading, not just naming: one row
    /// each, with the sentence the plug-in wrote and a link to where its
    /// makers describe it.
    ///
    /// The link's text is the address. A plug-in supplying a destination
    /// is a third party deciding where PolterType sends somebody, so
    /// what is clicked has to be what is read: a friendly label over an
    /// arbitrary URL is the exact shape this pane's draw-it-ourselves
    /// rule exists to prevent. `validate` has already refused anything
    /// that is not `https`.
    fn plugin_choice_cards<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
        chosen: Option<String>,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let mut list = Column::new().spacing(10).width(Length::Fill);

        for (slot, option) in control.options.iter().enumerate() {
            let picked = chosen.as_deref() == Some(option.value());
            // A radio rather than a checkbox: a row of tick-boxes reads
            // as "any of these", the wrong promise for alternatives.
            // Keyed on the row's position because the widget wants a
            // `Copy` value and the choice is a string.
            let mut entry = Column::new().spacing(3).push(
                iced::widget::Radio::new(option.label(), slot, picked.then_some(slot), {
                    let v = option.value().to_owned();
                    move |_| Message::PluginChoiceSelected(plugin, index, v.clone())
                })
                .text_size(13)
                .size(15),
            );
            if !option.detail().trim().is_empty() {
                entry = entry.push(
                    Row::new()
                        .push(Space::new().width(Length::Fixed(26.0)))
                        .push(
                            Text::new(option.detail())
                                .size(11)
                                .color(b.muted)
                                .width(Length::Fill),
                        ),
                );
            }
            let link = option.link().trim();
            if !link.is_empty() {
                entry = entry.push(
                    Row::new()
                        .push(Space::new().width(Length::Fixed(22.0)))
                        .push(
                            Button::new(Text::new(link.to_owned()).size(11))
                                .style(theme::link)
                                .padding(Padding {
                                    top: 0.0,
                                    right: 4.0,
                                    bottom: 0.0,
                                    left: 4.0,
                                })
                                .on_press(Message::PluginOpenLink(link.to_owned())),
                        ),
                );
            }
            list = list.push(entry);
        }

        self.described(control)
            .spacing(8)
            .push(
                Container::new(list)
                    .style(theme::card)
                    .padding(12)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .into()
    }

    /// A list: one checkbox per row the plug-in supplied, each ticking
    /// its own name into an array in the plug-in's config. A row
    /// contributes a name, a label and a line of detail — nothing about
    /// how any of it looks.
    fn plugin_list<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        // "Select all" means nothing before the rows exist, so the
        // buttons appear with the boxes they act on.
        let has_rows = !pane.list_rows(Slot::control(index)).is_empty();
        let column = self.output_heading_with(plugin, index, control, has_rows);

        let body: Element<'a, Message> = match pane.output(Slot::control(index)) {
            None | Some(CommandOutput::Loading) => {
                Text::new(tr("plugins.asking", "Asking the plug-in…"))
                    .size(12)
                    .color(b.muted)
                    .into()
            }
            Some(CommandOutput::Failed(why)) => Text::new(tr_args(
                "plugins.could_not_ask",
                "Could not ask the plug-in: {}",
                &[why.as_str()],
            ))
            .size(12)
            .color(b.warn)
            .width(Length::Fill)
            .into(),
            Some(CommandOutput::Ready(_)) => {
                let rows = pane.list_rows(Slot::control(index));
                if rows.is_empty() {
                    Text::new(tr(
                        "plugins.nothing_to_choose",
                        "The plug-in offered nothing to choose from.",
                    ))
                    .size(12)
                    .color(b.muted)
                    .into()
                } else {
                    let mut list = Column::new().spacing(9).width(Length::Fill);
                    for row in rows {
                        let ticked = pane.in_array(index, &row.id);
                        let id = row.id.clone();
                        let mut entry = Column::new().spacing(2).push(
                            Checkbox::new(ticked)
                                .label(row.label.as_str())
                                .text_size(13)
                                .on_toggle(move |on| {
                                    Message::PluginListToggled(plugin, index, id.clone(), on)
                                }),
                        );
                        if !row.detail.is_empty() {
                            // Indented under its own box, so the detail
                            // reads as belonging to that row rather
                            // than to the next one.
                            entry = entry.push(
                                Row::new()
                                    .push(Space::new().width(Length::Fixed(26.0)))
                                    .push(
                                        Text::new(row.detail.clone())
                                            .size(11)
                                            .color(b.muted)
                                            .width(Length::Fill),
                                    ),
                            );
                        }
                        list = list.push(entry);
                    }
                    list.into()
                }
            }
        };

        column
            .push(
                Container::new(body)
                    .style(theme::card)
                    .padding(12)
                    .width(Length::Fill),
            )
            .into()
    }

    /// A report: the plug-in's own answer, shown as text.
    ///
    /// Fixed-width and boxed on purpose: this is the one place a
    /// plug-in's *output* reaches the settings window, so it has to read
    /// as a quotation rather than as something PolterType is saying —
    /// no styling of its own, no controls drawn from it, and a heading
    /// naming what produced it.
    fn plugin_report<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let state = self.plugins[plugin].output(Slot::control(index));
        let body: Element<'a, Message> = match state {
            None | Some(CommandOutput::Loading) => {
                Text::new(tr("plugins.asking", "Asking the plug-in…"))
                    .size(12)
                    .color(b.muted)
                    .into()
            }
            Some(CommandOutput::Ready(text)) if text.trim().is_empty() => Text::new(tr(
                "plugins.nothing_to_report",
                "The plug-in had nothing to report.",
            ))
            .size(12)
            .color(b.muted)
            .into(),
            Some(CommandOutput::Ready(text)) => Text::new(text.as_str())
                .size(12)
                .font(FONT_MONO)
                .width(Length::Fill)
                .into(),
            // Said plainly rather than left as an empty box, which
            // would read as "nothing to say".
            Some(CommandOutput::Failed(why)) => Text::new(tr_args(
                "plugins.could_not_ask",
                "Could not ask the plug-in: {}",
                &[why.as_str()],
            ))
            .size(12)
            .color(b.warn)
            .width(Length::Fill)
            .into(),
        };

        self.output_heading(plugin, index, control)
            .push(
                Container::new(body)
                    .style(theme::card)
                    .padding(12)
                    .width(Length::Fill),
            )
            .into()
    }

    /// Title, refresh button and explanation for a control whose
    /// contents come from the plug-in. The explanation sits above the
    /// box, not below it: under a 340-pixel list it belongs to nothing.
    fn output_heading<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
    ) -> Column<'a, Message> {
        self.output_heading_with(plugin, index, control, false)
    }

    /// The same, with the two batch buttons a tick-box list needs.
    ///
    /// Only a list gets them, and only once it has rows: sixty offered
    /// conversations is sixty boxes to tick by hand. Not on a report,
    /// which has nothing to tick, and not on an empty list, where they
    /// would be two dead controls.
    fn output_heading_with<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a PaneControl,
        batch: bool,
    ) -> Column<'a, Message> {
        let b = self.brand();
        let small = Padding {
            top: 3.0,
            right: 9.0,
            bottom: 3.0,
            left: 9.0,
        };
        let mut heading = Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(Text::new(control.label.as_str()).size(13).font(font_bold()))
            .push(
                Button::new(Text::new(tr("plugins.refresh", "Refresh")).size(11))
                    .padding(small)
                    .on_press(Message::PluginOutputRefresh(plugin, Slot::control(index))),
            );
        if batch {
            heading = heading
                .push(
                    Button::new(Text::new(tr("plugins.select_all", "Select all")).size(11))
                        .padding(small)
                        .style(iced::widget::button::secondary)
                        .on_press(Message::PluginListAll(plugin, index, true)),
                )
                .push(
                    Button::new(Text::new(tr("plugins.clear", "Clear")).size(11))
                        .padding(small)
                        .style(iced::widget::button::secondary)
                        .on_press(Message::PluginListAll(plugin, index, false)),
                );
        }
        let mut column = Column::new().spacing(6).push(heading);
        if !control.help.is_empty() {
            column = column.push(
                Text::new(control.help.as_str())
                    .size(11)
                    .color(b.muted)
                    .width(Length::Fill),
            );
        }
        column
    }
}

/// A choice as a drop-down.
///
/// What is *read* is the option's label and what is *written* is its
/// value, and after a plug-in's translation has been applied the two
/// are rarely the same string — so the mapping happens here rather than
/// showing a config value to somebody as if it were a word.
fn choice_picker(
    control: &PaneControl,
    stored: Option<String>,
    text_size: u32,
    on_pick: impl Fn(String) -> Message + 'static,
) -> Element<'static, Message> {
    let pairs: Vec<(String, String)> = control
        .options
        .iter()
        .map(|o| (o.label().to_owned(), o.value().to_owned()))
        .collect();
    let labels: Vec<String> = pairs.iter().map(|(label, _)| label.clone()).collect();
    // A stored value with no option of its own is shown as itself: the
    // plug-in's config said something this manifest no longer offers,
    // and blanking the box would quietly propose overwriting it.
    let selected = stored.map(|value| {
        pairs
            .iter()
            .find(|(_, declared)| *declared == value)
            .map(|(label, _)| label.clone())
            .unwrap_or(value)
    });

    PickList::new(labels, selected, move |chosen| {
        let value = pairs
            .iter()
            .find(|(label, _)| *label == chosen)
            .map(|(_, value)| value.clone())
            .unwrap_or(chosen);
        on_pick(value)
    })
    .text_size(text_size)
    .placeholder(tr("plugins.default", PLUGIN_DEFAULT))
    .width(Length::Fill)
    .into()
}
