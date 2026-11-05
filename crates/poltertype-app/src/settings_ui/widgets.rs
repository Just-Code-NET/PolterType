//! Building blocks every pane is drawn from.
//!
//! The one rule behind them: **a control is never squeezed.** iced hands
//! a row's leftover width to whichever child comes last, and a button
//! given too little breaks its label into a column of single words. So
//! labels here do not wrap, runs of buttons wrap as a whole onto the next
//! line, and a form row falls back to stacking when it runs out of room.

use iced::widget::{Button, Checkbox, Column, Container, Row, Text, text};
use iced::{Alignment, Color, Element, Font, Length, Padding};

use super::enums::Message;
use super::state::SaveBanner;
use super::theme::{self, font_bold};

/// A button's label: one line, whatever the window's width.
pub(super) fn button_label<'a>(label: impl Into<String>, size: u32) -> Text<'a> {
    Text::new(label.into())
        .size(size)
        .wrapping(text::Wrapping::None)
}

/// The window's ordinary quiet button. `None` draws it disabled.
pub(super) fn action<'a>(
    label: impl Into<String>,
    on_press: Option<Message>,
) -> Button<'a, Message> {
    Button::new(button_label(label, 12))
        .style(theme::secondary)
        .padding(Padding {
            top: 5.0,
            right: 12.0,
            bottom: 5.0,
            left: 12.0,
        })
        .on_press_maybe(on_press)
}

/// One alternative of a segmented choice.
pub(super) fn chip<'a>(
    label: impl Into<String>,
    selected: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    action(label, on_press).style(theme::chip(selected)).into()
}

/// A run of buttons or chips that moves onto a new line rather than
/// shrinking any of them.
pub(super) fn button_run<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    Row::with_children(items)
        .spacing(8)
        .wrap()
        .vertical_spacing(8)
        .into()
}

/// What a setting is on the left with its explanation under it, its value
/// on the right. `stacked` puts the value underneath, for a window with no
/// room for two columns.
pub(super) fn form_row<'a>(
    b: &'static theme::BrandPalette,
    stacked: bool,
    label: impl Into<String>,
    hint: impl Into<String>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    let hint = hint.into();
    let mut described = Column::new()
        .spacing(3)
        .width(Length::Fill)
        .push(Text::new(label.into()).size(13).color(b.ink));
    if !hint.is_empty() {
        described = described.push(Text::new(hint).size(11).color(b.muted));
    }
    if stacked {
        return described.spacing(6).push(control).into();
    }
    Row::new()
        .spacing(20)
        .align_y(Alignment::Center)
        .push(described)
        .push(control)
        .into()
}

/// A setting whose alternatives need the whole width: name, alternatives,
/// then the sentence about them.
pub(super) fn choice_block<'a>(
    b: &'static theme::BrandPalette,
    label: impl Into<String>,
    hint: impl Into<String>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    let hint = hint.into();
    let mut column = Column::new()
        .spacing(6)
        .width(Length::Fill)
        .push(Text::new(label.into()).size(13).color(b.ink))
        .push(control);
    if !hint.is_empty() {
        column = column.push(Text::new(hint).size(11).color(b.muted));
    }
    column.into()
}

/// A plain switch with its label.
pub(super) fn switch<'a>(
    on: bool,
    label: impl Into<String>,
    on_toggle: fn(bool) -> Message,
) -> Element<'a, Message> {
    Checkbox::new(on)
        .label(label.into())
        .text_size(13)
        .on_toggle(on_toggle)
        .into()
}

/// `[−] value [+]`. The buttons go inert when `enabled` is off, which is
/// how a dependent setting says it does not apply right now.
pub(super) fn stepper<'a>(
    enabled: bool,
    value: String,
    color: Color,
    down: Message,
    up: Message,
) -> Element<'a, Message> {
    let step = |label: &'static str, msg: Message| {
        Button::new(button_label(label, 13))
            .style(theme::secondary)
            .padding(Padding {
                top: 4.0,
                right: 11.0,
                bottom: 4.0,
                left: 11.0,
            })
            .on_press_maybe(enabled.then_some(msg))
    };
    Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .push(step("−", down))
        .push(
            Container::new(
                Text::new(value)
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(color)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(62.0))
            .align_x(Alignment::Center),
        )
        .push(step("+", up))
        .into()
}

/// A line between two groups inside one card.
pub(super) fn divider<'a>() -> Element<'a, Message> {
    iced::widget::rule::horizontal(1)
        .style(theme::hairline)
        .into()
}

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

/// Muted footnote at the bottom of a pane.
pub(super) fn tip(
    b: &'static theme::BrandPalette,
    text: impl Into<String>,
) -> Element<'static, Message> {
    Text::new(text.into()).size(11).color(b.muted).into()
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
