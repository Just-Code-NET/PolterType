//! The Plug-ins pane: what is installed, and its own settings.
//!
//! Every control on this pane is drawn by us from a static
//! declaration. A plug-in never renders anything — which is what stops
//! a third-party pane from imitating a system prompt, PolterType's own
//! dialogs, or another plug-in.
//!
//! Two things the pane says out loud rather than hiding:
//!
//! * **where a plug-in came from.** One that was found in a checkout
//!   or on `POLTERTYPE_PLUGIN_PATH` rather than installed is running
//!   code nobody installed, and the user should be able to see that.
//! * **which file is being edited.** These settings do not live in
//!   PolterType's config; they live in the plug-in's, and an edit here
//!   is an edit to a file another program owns.

use iced::widget::{
    Button, Checkbox, Column, Container, PickList, Row, Scrollable, Space, Text, TextInput,
};
use iced::{Alignment, Element, Length};
use poltertype_core::plugins::{ControlKind, SettingValue};

use super::enums::*;
use super::plugin_pane::ReportState;
use super::state::*;
use super::theme::FONT_BOLD;
use super::theme::FONT_MONO;
use super::view::section_title;

/// Shown where a plug-in's config does not set a value. Not "0" and
/// not an empty selection that looks chosen: the plug-in has a default
/// and this pane does not know it.
const PLUGIN_DEFAULT: &str = "(plug-in default)";

/// How tall a report block may grow before it scrolls inside itself.
/// Enough for a dozen lines — the pane has its own scrolling, and a
/// report that pushed every other control off the screen would be a
/// plug-in deciding the layout of somebody else's window.
const REPORT_HEIGHT: f32 = 220.0;

impl SettingsApp {
    pub(super) fn view_plugins(&self) -> Element<'_, Message> {
        let b = self.brand();
        let mut body = Column::new().spacing(18);

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
                        .size(13),
                    ),
            )
            .padding(20)
            .into();
        }

        body = body.push(section_title(b, "Plug-ins"));

        for (plugin_index, plugin) in self.plugins.iter().enumerate() {
            let mut card = Column::new().spacing(10);

            let mut heading = Row::new().spacing(8).align_y(Alignment::Center).push(
                Text::new(format!("{} {}", plugin.ext.name, plugin.ext.version))
                    .size(15)
                    .font(FONT_BOLD),
            );
            if plugin.ext.development {
                // Not a badge of honour: this is code that was never
                // installed, found next to a source checkout.
                heading = heading.push(Text::new("· development build").size(12));
            }
            card = card.push(heading);

            if !plugin.ext.manifest.summary.is_empty() {
                card = card.push(Text::new(&plugin.ext.manifest.summary).size(13));
            }
            card = card.push(
                Text::new(format!("Settings file: {}", plugin.config_path.display())).size(11),
            );

            for (control_index, control) in plugin.ext.manifest.pane.iter().enumerate() {
                card = card.push(self.plugin_control(plugin_index, control_index, control));
            }

            if let Some(status) = &plugin.status {
                card = card.push(Text::new(status).size(11));
            }

            body = body
                .push(Container::new(card).padding(14).width(Length::Fill))
                .push(Space::with_height(4));
        }

        Container::new(body).padding(20).into()
    }

    /// One declared control, rendered natively.
    fn plugin_control<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Element<'a, Message> {
        let pane = &self.plugins[plugin];
        // The *stored* value, which may be absent — the plug-in then
        // applies its own default, and we do not know what that is.
        // Showing a fabricated 0 or a blank choice as though the user
        // had chosen it would be the pane lying about the config.
        let stored = pane.values.get(index).and_then(|v| v.clone());
        let value = pane.value_of(index);

        let widget: Element<'a, Message> = match control.kind {
            ControlKind::Toggle => Checkbox::new(
                control.label.as_str(),
                matches!(value, SettingValue::Bool(true)),
            )
            .text_size(13)
            .on_toggle(move |on| Message::PluginToggled(plugin, index, on))
            .into(),

            ControlKind::Choice => Row::new()
                .spacing(10)
                .align_y(Alignment::Center)
                .push(Text::new(control.label.as_str()).size(13))
                .push(
                    PickList::new(
                        control.options.clone(),
                        stored.as_ref().map(SettingValue::as_display),
                        move |chosen| Message::PluginChoiceSelected(plugin, index, chosen),
                    )
                    .placeholder(PLUGIN_DEFAULT),
                )
                .into(),

            ControlKind::Text | ControlKind::Number => Row::new()
                .spacing(10)
                .align_y(Alignment::Center)
                .push(Text::new(control.label.as_str()).size(13))
                .push(
                    TextInput::new(
                        PLUGIN_DEFAULT,
                        &stored
                            .as_ref()
                            .map(SettingValue::as_display)
                            .unwrap_or_default(),
                    )
                    .size(13)
                    .on_input(move |text| Message::PluginTextChanged(plugin, index, text)),
                )
                .into(),

            ControlKind::Button => Button::new(Text::new(control.label.as_str()).size(13))
                .on_press(Message::PluginCommandClicked(
                    plugin,
                    control.command.clone(),
                ))
                .into(),

            ControlKind::Report => self.plugin_report(plugin, index, control),

            // Said plainly, in place of the control. The alternative —
            // rendering nothing — leaves a plug-in looking like it
            // forgot half its settings.
            ControlKind::Unknown => Text::new(format!(
                "“{}” needs a newer version of PolterType.",
                control.label
            ))
            .size(12)
            .into(),
        };

        let mut row = Column::new().spacing(3).push(widget);
        if !control.help.is_empty() {
            row = row.push(Text::new(control.help.as_str()).size(11));
        }
        row.into()
    }

    /// A report: the plug-in's own answer, shown as text.
    ///
    /// Fixed-width and inside the plug-in's card on purpose. This is
    /// the one place a plug-in's *output* reaches the settings window,
    /// so it has to be unmistakably a quotation rather than something
    /// PolterType is saying: no styling of its own, no controls drawn
    /// from it, and a heading above it naming what produced it.
    fn plugin_report<'a>(
        &'a self,
        plugin: usize,
        index: usize,
        control: &'a poltertype_core::plugins::PaneControl,
    ) -> Element<'a, Message> {
        let state = self.plugins[plugin].reports.get(&index);
        let body: Element<'a, Message> = match state {
            None | Some(ReportState::Loading) => Text::new("Asking the plug-in…").size(12).into(),
            Some(ReportState::Ready(text)) if text.trim().is_empty() => {
                Text::new("The plug-in had nothing to report.")
                    .size(12)
                    .into()
            }
            Some(ReportState::Ready(text)) => Scrollable::new(
                Text::new(text.as_str())
                    .size(12)
                    .font(FONT_MONO)
                    .width(Length::Fill),
            )
            .height(Length::Fixed(REPORT_HEIGHT))
            .into(),
            // Said plainly rather than left as an empty box: a plug-in
            // that cannot answer is worth seeing, and "nothing here"
            // would read as "nothing to say".
            Some(ReportState::Failed(why)) => {
                Text::new(format!("Could not ask the plug-in: {why}"))
                    .size(12)
                    .into()
            }
        };

        Column::new()
            .spacing(6)
            .push(
                Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(Text::new(control.label.as_str()).size(13).font(FONT_BOLD))
                    .push(
                        Button::new(Text::new("Refresh").size(12))
                            .on_press(Message::PluginReportRefresh(plugin, index)),
                    ),
            )
            .push(Container::new(body).padding(10).width(Length::Fill))
            .into()
    }
}
