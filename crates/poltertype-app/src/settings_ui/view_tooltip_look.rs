//! The Suggestions pane's "Size and position" card: how large the
//! tooltip draws (issue #79) and where in the focused window it sits
//! when it is not at the text cursor (issue #81).
//!
//! Its own file so the pane's other cards can change without this one
//! moving under them. Every control here is read per tooltip by the
//! tray, so a saved change shows on the next one — nothing to restart.

use iced::widget::{Button, Column, Container, Row, Space, Text};
use iced::{Alignment, Element, Font, Length, Padding};
use poltertype_core::i18n::tr;
use poltertype_core::settings::{SuggestionSettings, TooltipSpot};

use super::enums::*;
use super::state::*;
use super::theme;
use super::view::section_title;

/// The picker's cells, row by row as they appear on screen.
const GRID: [[TooltipSpot; 3]; 3] = [
    [
        TooltipSpot::TopLeft,
        TooltipSpot::TopCenter,
        TooltipSpot::TopRight,
    ],
    [
        TooltipSpot::MiddleLeft,
        TooltipSpot::Center,
        TooltipSpot::MiddleRight,
    ],
    [
        TooltipSpot::BottomLeft,
        TooltipSpot::BottomCenter,
        TooltipSpot::BottomRight,
    ],
];

impl SettingsApp {
    pub(super) fn tooltip_look_card(&self) -> Element<'_, Message> {
        let b = self.brand();
        let s = &self.settings.suggestions;
        let value_color = if s.enabled { b.ink } else { b.muted };

        let size = stepper(
            s.enabled,
            format!("{}%", (s.tooltip_scale() * 100.0).round()),
            value_color,
            Message::SuggestionScaleDelta(-25),
            Message::SuggestionScaleDelta(25),
        );
        let margin = stepper(
            s.enabled,
            format!("{} px", s.window_margin_clamped()),
            value_color,
            Message::SuggestionMarginDelta(-8),
            Message::SuggestionMarginDelta(8),
        );

        Column::new()
            .spacing(14)
            .push(section_title(
                b,
                tr("suggestions.size_and_position", "Size and position"),
            ))
            .push(form_row(
                b,
                tr("suggestions.size", "Size"),
                tr(
                    "suggestions.size_hint",
                    "Text, spacing and panel together, on top of the desktop's own \
                     scaling. 100% is the designed size.",
                ),
                size,
            ))
            .push(form_row(
                b,
                tr("suggestions.window_spot", "Place in the window"),
                tr(
                    "suggestions.window_spot_hint",
                    "Where the tooltip sits when it is not at the text cursor: always, \
                     with the cursor option above off; otherwise whenever the app reports \
                     no cursor. It never leaves the screen.",
                ),
                spot_picker(b, s),
            ))
            .push(form_row(
                b,
                tr("suggestions.window_margin", "Distance from the edge"),
                tr(
                    "suggestions.window_margin_hint",
                    "Gap between the tooltip and each window edge its place touches.",
                ),
                margin,
            ))
            .into()
    }
}

/// What the setting is on the left with its explanation under it, its
/// value on the right — the window's form-row shape.
fn form_row<'a>(
    b: &'static theme::BrandPalette,
    label: &'static str,
    hint: &'static str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    Row::new()
        .spacing(16)
        .align_y(Alignment::Center)
        .push(
            Column::new()
                .spacing(3)
                .width(Length::Fill)
                .push(Text::new(label).size(13))
                .push(Text::new(hint).size(11).color(b.muted)),
        )
        .push(control)
        .into()
}

/// `[−] value [+]`, with the buttons inert while suggestions are off —
/// the same signal the pane's other steppers give.
fn stepper<'a>(
    enabled: bool,
    value: String,
    color: iced::Color,
    down: Message,
    up: Message,
) -> Element<'a, Message> {
    let step = |label: &'static str, msg: Message| {
        let btn = Button::new(Text::new(label).size(12))
            .style(theme::secondary)
            .padding(Padding {
                top: 4.0,
                right: 10.0,
                bottom: 4.0,
                left: 10.0,
            });
        if enabled { btn.on_press(msg) } else { btn }
    };
    Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(step("−", down))
        .push(
            Container::new(Text::new(value).size(13).font(Font::MONOSPACE).color(color))
                .width(Length::Fixed(56.0))
                .align_x(Alignment::Center),
        )
        .push(step("+", up))
        .into()
}

/// A miniature window: nine cells, the chosen one filled, its name
/// underneath so the choice reads without decoding the grid.
fn spot_picker<'a>(
    b: &'static theme::BrandPalette,
    s: &SuggestionSettings,
) -> Element<'a, Message> {
    let chosen = s.window_spot();
    let mut grid = Column::new().spacing(4);
    for row in GRID {
        let mut cells = Row::new().spacing(4);
        for spot in row {
            let mut cell = Button::new(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fixed(30.0))
                .height(Length::Fixed(20.0))
                .style(theme::chip(spot == chosen));
            if s.enabled {
                cell = cell.on_press(Message::SuggestionWindowSpotChosen(spot));
            }
            cells = cells.push(cell);
        }
        grid = grid.push(cells);
    }
    Column::new()
        .spacing(6)
        .align_x(Alignment::Center)
        .push(Container::new(grid).style(theme::card).padding(6))
        .push(Text::new(spot_name(chosen)).size(11).color(b.muted))
        .into()
}

fn spot_name(spot: TooltipSpot) -> &'static str {
    match spot {
        TooltipSpot::TopLeft => tr("suggestions.spot_top_left", "Top left"),
        TooltipSpot::TopCenter => tr("suggestions.spot_top_center", "Top centre"),
        TooltipSpot::TopRight => tr("suggestions.spot_top_right", "Top right"),
        TooltipSpot::MiddleLeft => tr("suggestions.spot_middle_left", "Middle left"),
        TooltipSpot::Center => tr("suggestions.spot_center", "Centre"),
        TooltipSpot::MiddleRight => tr("suggestions.spot_middle_right", "Middle right"),
        TooltipSpot::BottomLeft => tr("suggestions.spot_bottom_left", "Bottom left"),
        TooltipSpot::BottomCenter => tr("suggestions.spot_bottom_center", "Bottom centre"),
        TooltipSpot::BottomRight => tr("suggestions.spot_bottom_right", "Bottom right"),
    }
}
