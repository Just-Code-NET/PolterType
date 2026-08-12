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
//! A plug-in with a hundred settings — which is what a capable one has
//! — cannot be one column. Sections become a second navigation list
//! beside the window's own, one section on screen at a time, and every
//! control is laid out on the same two-column grid so the eye has one
//! place to find a label and one place to find its value.
//!
//! **Exactly one thing on this pane scrolls.** A room list or a report
//! inside its own scrolling box put a second bar a few pixels from the
//! first, and a wheel over the boundary then moves whichever one the
//! pointer happened to be over. So those grow to their content and the
//! page scrolls once.

use iced::widget::{
    Button, Checkbox, Column, Container, PickList, Row, Scrollable, Space, Text, TextInput,
    horizontal_rule,
};
use iced::{Alignment, Element, Length, Padding};
use poltertype_core::plugins::{ControlKind, SettingValue};

use super::enums::*;
use super::plugin_pane::CommandOutput;
use super::state::*;
use super::theme::{self, FONT_BOLD, FONT_MONO};
use super::view::section_title;

/// Shown where a plug-in's config does not set a value. Not "0" and
/// not an empty selection that looks chosen: the plug-in has a default
/// and this pane does not know it.
const PLUGIN_DEFAULT: &str = "(plug-in default)";

/// Placeholder for a list typed by hand, saying how to separate the
/// members. The alternative is a user discovering the rule by having
/// their spaces silently become part of a name.
const PLUGIN_LIST_HINT: &str = "(empty — separate with commas)";

/// The same for a number box, which is too narrow to say it in full.
const PLUGIN_DEFAULT_SHORT: &str = "default";

/// Width of the value column. Every switch, picker and number lands on
/// the same right-hand edge — which is the whole difference between a
/// form and a list of sentences with boxes after them.
const VALUE_COLUMN: f32 = 210.0;

/// Gap between what a setting is and what it is set to.
const LABEL_GAP: f32 = 24.0;

/// Width of the section list. Narrower than the window's own nav so the
/// two do not read as one two-level menu of equal weight.
const SECTION_NAV: f32 = 186.0;

/// How wide a number gets. A box sized for a paragraph invites one.
const NUMBER_WIDTH: f32 = 110.0;

impl SettingsApp {
    pub(super) fn view_plugins(&self) -> Element<'_, Message> {
        let b = self.brand();

        if self.plugins.is_empty() {
            return Container::new(
                Column::new()
                    .spacing(10)
                    .push(section_title(b, "Plug-ins"))
                    .push(
                        Text::new(
                            "No plug-ins are installed. A plug-in is a separate program that \
                             PolterType runs and shows here; it is never loaded into PolterType \
                             itself.",
                        )
                        .size(13)
                        .color(b.muted),
                    ),
            )
            .padding(20)
            .into();
        }

        // No padding and no card of its own: the window already pads
        // every pane, and a second frame inside the first is what made
        // this page sit further from the edge than every other one.
        let mut body = Column::new().spacing(12).push(section_title(b, "Plug-ins"));
        for (index, _) in self.plugins.iter().enumerate() {
            if index > 0 {
                body = body.push(horizontal_rule(1).style(theme::hairline));
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
                .font(FONT_BOLD),
        );
        if pane.ext.development {
            // Not a badge of honour: this is code that was never
            // installed, found next to a source checkout.
            heading = heading.push(Text::new("· development build").size(12).color(b.warn));
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
            Text::new(format!("Settings file: {}", pane.config_path.display()))
                .size(11)
                .color(b.muted),
        );

        let sections = pane.sections();
        // Only one scrolling region on the page. When this pane owns
        // the window's height it is the settings column, so the section
        // list stays put beside it; when several plug-ins share the
        // pane the window's own scrollbar is the one, and nothing here
        // adds a second.
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
            .push(horizontal_rule(1).style(theme::hairline))
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
        // Buttons declared next to each other are one row of actions.
        // Stacked, three of them cost a third of the page and read as
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
        control: &'a poltertype_core::plugins::PaneControl,
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
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        // The *stored* value, which may be absent — the plug-in then
        // applies its own default, and we do not know what that is.
        // Showing a fabricated 0 or a blank choice as though the user
        // had chosen it would be the pane lying about the config.
        let stored = pane.values.get(index).and_then(|v| v.clone());
        let value = pane.value_of(index);
        let typed = pane.display_of(index).unwrap_or_default();

        match control.kind {
            // The heading of the section being shown. Repeated here
            // rather than only in the nav: the nav says where you are
            // among thirteen, and this says what you are looking at,
            // with the sentence that explains it.
            ControlKind::Section => {
                let mut head = Column::new().spacing(6).push(
                    Text::new(control.label.as_str())
                        .size(15)
                        .font(FONT_BOLD)
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
                head.push(horizontal_rule(1).style(theme::hairline)).into()
            }

            ControlKind::Toggle => self.field(
                control,
                Checkbox::new("", matches!(value, SettingValue::Bool(true)))
                    .on_toggle(move |on| Message::PluginToggled(plugin, index, on))
                    .into(),
            ),

            ControlKind::Choice => self.field(
                control,
                PickList::new(
                    control.options.clone(),
                    stored.as_ref().map(SettingValue::as_display),
                    move |chosen| Message::PluginChoiceSelected(plugin, index, chosen),
                )
                .text_size(13)
                .placeholder(PLUGIN_DEFAULT)
                .width(Length::Fill)
                .into(),
            ),

            ControlKind::Number | ControlKind::Decimal => self.field(
                control,
                Row::new()
                    .push(Space::with_width(Length::Fill))
                    .push(
                        TextInput::new(PLUGIN_DEFAULT_SHORT, &typed)
                            .size(13)
                            .align_x(Alignment::End)
                            .width(Length::Fixed(NUMBER_WIDTH))
                            .on_input(move |text| Message::PluginTextChanged(plugin, index, text)),
                    )
                    .into(),
            ),

            // Wide by itself: an endpoint URL or a list of host names
            // does not fit in the value column, and a box you have to
            // scroll sideways to read is worse than a wider row.
            ControlKind::Text | ControlKind::Strings => self.wide_field(
                control,
                TextInput::new(
                    if control.kind == ControlKind::Strings {
                        PLUGIN_LIST_HINT
                    } else {
                        PLUGIN_DEFAULT
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

            // Said plainly, in place of the control. The alternative —
            // rendering nothing — leaves a plug-in looking like it
            // forgot half its settings.
            ControlKind::Unknown => Text::new(format!(
                "“{}” needs a newer version of PolterType.",
                control.label
            ))
            .size(12)
            .color(b.warn)
            .into(),
        }
    }

    /// One row of the form: what the setting is on the left, with its
    /// explanation under it, and what it is set to on the right.
    ///
    /// The explanation belongs on the wide side. Under the *control* it
    /// gets a column two hundred pixels across, and a paragraph in a
    /// column that narrow is a stack of three-word lines that pushes
    /// every following row down the page.
    fn field<'a>(
        &'a self,
        control: &'a poltertype_core::plugins::PaneControl,
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
        control: &'a poltertype_core::plugins::PaneControl,
        widget: Element<'a, Message>,
    ) -> Element<'a, Message> {
        self.described(control)
            .spacing(6)
            .push(widget)
            .width(Length::Fill)
            .into()
    }

    /// A setting's name, and the sentence explaining it.
    fn described<'a>(
        &'a self,
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Column<'a, Message> {
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

    /// A list: one checkbox per row the plug-in supplied, each ticking
    /// its own name into an array in the plug-in's config.
    ///
    /// It still draws only boxes — a row contributes a name, a label
    /// and a line of detail, and nothing about how any of it looks.
    fn plugin_list<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let pane = &self.plugins[plugin];
        // Rows have to exist before "select all" means anything, and
        // while the plug-in is still being asked there is nothing to act
        // on — so the buttons appear with the boxes they act on.
        let has_rows = !pane.list_rows(index).is_empty();
        let column = self.output_heading_with(plugin, index, control, has_rows);

        let body: Element<'a, Message> = match pane.output(index) {
            None | Some(CommandOutput::Loading) => Text::new("Asking the plug-in…")
                .size(12)
                .color(b.muted)
                .into(),
            Some(CommandOutput::Failed(why)) => {
                Text::new(format!("Could not ask the plug-in: {why}"))
                    .size(12)
                    .color(b.warn)
                    .width(Length::Fill)
                    .into()
            }
            Some(CommandOutput::Ready(_)) => {
                let rows = pane.list_rows(index);
                if rows.is_empty() {
                    Text::new("The plug-in offered nothing to choose from.")
                        .size(12)
                        .color(b.muted)
                        .into()
                } else {
                    let mut list = Column::new().spacing(9).width(Length::Fill);
                    for row in rows {
                        let ticked = pane.in_array(index, &row.id);
                        let id = row.id.clone();
                        let mut entry = Column::new().spacing(2).push(
                            Checkbox::new(row.label.as_str(), ticked)
                                .text_size(13)
                                .on_toggle(move |on| {
                                    Message::PluginListToggled(plugin, index, id.clone(), on)
                                }),
                        );
                        if !row.detail.is_empty() {
                            // Indented under its own box, so a row
                            // saying what was measured about an
                            // application reads as belonging to it
                            // rather than to the next one.
                            entry = entry.push(
                                Row::new()
                                    .push(Space::with_width(Length::Fixed(26.0)))
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
    /// Fixed-width and inside a box of its own on purpose. This is the
    /// one place a plug-in's *output* reaches the settings window, so it
    /// has to be unmistakably a quotation rather than something
    /// PolterType is saying: no styling of its own, no controls drawn
    /// from it, and a heading above it naming what produced it.
    fn plugin_report<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Element<'a, Message> {
        let b = self.brand();
        let state = self.plugins[plugin].output(index);
        let body: Element<'a, Message> = match state {
            None | Some(CommandOutput::Loading) => Text::new("Asking the plug-in…")
                .size(12)
                .color(b.muted)
                .into(),
            Some(CommandOutput::Ready(text)) if text.trim().is_empty() => {
                Text::new("The plug-in had nothing to report.")
                    .size(12)
                    .color(b.muted)
                    .into()
            }
            Some(CommandOutput::Ready(text)) => Text::new(text.as_str())
                .size(12)
                .font(FONT_MONO)
                .width(Length::Fill)
                .into(),
            // Said plainly rather than left as an empty box: a plug-in
            // that cannot answer is worth seeing, and "nothing here"
            // would read as "nothing to say".
            Some(CommandOutput::Failed(why)) => {
                Text::new(format!("Could not ask the plug-in: {why}"))
                    .size(12)
                    .color(b.warn)
                    .width(Length::Fill)
                    .into()
            }
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
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Column<'a, Message> {
        self.output_heading_with(plugin, index, control, false)
    }

    /// The same, with the two batch buttons a tick-box list needs.
    ///
    /// Only a list gets them, and only when it has rows: a plug-in can
    /// offer sixty conversations, and ticking sixty boxes by hand to say
    /// "all of them" is the kind of work a settings window exists to
    /// spare somebody. They are not on a report, which has nothing to
    /// tick, and not on an empty list, where they would be two dead
    /// controls explaining nothing.
    fn output_heading_with<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a poltertype_core::plugins::PaneControl,
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
            .push(Text::new(control.label.as_str()).size(13).font(FONT_BOLD))
            .push(
                Button::new(Text::new("Refresh").size(11))
                    .padding(small)
                    .on_press(Message::PluginOutputRefresh(plugin, index)),
            );
        if batch {
            heading = heading
                .push(
                    Button::new(Text::new("Select all").size(11))
                        .padding(small)
                        .style(iced::widget::button::secondary)
                        .on_press(Message::PluginListAll(plugin, index, true)),
                )
                .push(
                    Button::new(Text::new("Clear").size(11))
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
