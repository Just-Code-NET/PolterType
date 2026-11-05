//! The Suggestions pane: the tooltip, and how it is answered.

use iced::widget::{Checkbox, Column, Text, TextInput};
use iced::{Element, Length};
use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::settings::{AcceptKeys, SuggestionNavigation};

use super::enums::*;
use super::helpers::*;
use super::state::*;
use super::theme;
use super::widgets::{
    button_run, card, chip, choice_block, form_row, pane_header, section_title, stepper, switch,
    tip,
};

impl SettingsApp {
    pub(super) fn view_suggestions(&self) -> Element<'_, Message> {
        let b = self.brand();
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
            .push(card(self.suggestions_tooltip()))
            .push(card(self.tooltip_look_card()))
            .push(card(self.suggestions_keyboard()))
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

    fn suggestions_tooltip(&self) -> Column<'_, Message> {
        let b = self.brand();
        let s = &self.settings.suggestions;
        let value_color = if s.enabled { b.ink } else { b.muted };

        // A Checkbox with no `on_toggle` renders disabled, which is the
        // right signal while the tooltip itself is off: there is then
        // nothing to anchor anywhere.
        let mut anchor = Checkbox::new(s.caret_anchor && s.enabled)
            .label(tr(
                "suggestions.caret_anchor",
                "Place the tooltip at the text cursor",
            ))
            .text_size(13);
        if s.enabled {
            anchor = anchor.on_toggle(Message::SuggestionCaretAnchorToggled);
        }

        Column::new()
            .spacing(14)
            .push(section_title(b, tr("suggestions.tooltip", "Tooltip")))
            .push(switch(
                s.enabled,
                tr(
                    "suggestions.show_suggestions_mistyped_words",
                    "Show suggestions for mistyped words",
                ),
                Message::SuggestionsToggled,
            ))
            .push(form_row(
                b,
                self.stacked(),
                tr("suggestions.max_suggestions", "Max suggestions (1–9)"),
                tr(
                    "suggestions.each_entry_applied_with",
                    "Each entry is applied with one digit key, so 9 is the ceiling.",
                ),
                stepper(
                    s.enabled,
                    s.max_suggestions.to_string(),
                    value_color,
                    Message::SuggestionMaxDelta(-1),
                    Message::SuggestionMaxDelta(1),
                ),
            ))
            .push(form_row(
                b,
                self.stacked(),
                tr(
                    "suggestions.tooltip_timeout_seconds",
                    "Tooltip timeout (seconds)",
                ),
                tr(
                    "suggestions.seconds_tooltip_hides_itself",
                    "3–600 seconds; the tooltip hides itself when the time is up.",
                ),
                stepper(
                    s.enabled,
                    s.tooltip_timeout_secs.to_string(),
                    value_color,
                    Message::SuggestionTimeoutDelta(-5),
                    Message::SuggestionTimeoutDelta(5),
                ),
            ))
            .push(
                Column::new().spacing(4).push(anchor).push(
                    Text::new(tr(
                        "suggestions.caret_anchor_hint",
                        "Off, the tooltip always sits at the place in the window chosen below. \
                         On Linux this reaches outside PolterType: the cursor's position comes \
                         from the accessibility bus, and an application that joins that bus is \
                         what makes Qt ones — Telegram among them — announce that a screen reader \
                         is running. There, turning it back on takes effect on restart.",
                    ))
                    .size(11)
                    .color(b.muted),
                ),
            )
    }

    fn suggestions_keyboard(&self) -> Column<'_, Message> {
        let b = self.brand();
        let s = &self.settings.suggestions;
        let live = |msg: Message| s.enabled.then_some(msg);

        // F-keys and arrows only work where the listener can keep them
        // from the application — answered here without one, since this
        // window is its own process.
        let claimable = poltertype_input::suggestion_keys_claimable(self.settings.engine.hold_keys);
        let accept_keys = AcceptKeys::from_config(&s.accept_keys);
        let navigation = SuggestionNavigation::from_config(&s.navigation);
        let unavailable = |text: &'static str| Text::new(text).size(11).color(b.warn);

        let accept_row = button_run(
            [
                (
                    AcceptKeys::ModifierDigits,
                    tr("suggestions.accept_modifier_digits", "Modifiers + 1–9"),
                ),
                (
                    AcceptKeys::FunctionKeys,
                    tr("suggestions.accept_function_keys", "F1–F9"),
                ),
            ]
            .into_iter()
            .map(|(choice, label)| {
                chip(
                    label,
                    accept_keys == choice,
                    live(Message::SuggestionAcceptKeysChanged(choice)),
                )
            })
            .collect(),
        );
        // With F-keys in force the chord is not listened for and its
        // field goes; where F-keys cannot be claimed, the chord is what
        // still works, so it stays.
        let function_keys_live = accept_keys == AcceptKeys::FunctionKeys && claimable;

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

        let mut column = Column::new()
            .spacing(14)
            .push(section_title(
                b,
                tr("suggestions.keyboard_accept", "Keyboard accept"),
            ))
            .push(choice_block(
                b,
                tr("suggestions.accept_with", "Accept by number"),
                "",
                accept_row,
            ));
        if accept_keys == AcceptKeys::FunctionKeys {
            column = column.push(if claimable {
                Text::new(tr(
                    "suggestions.function_keys_hint",
                    "F1 applies the first suggestion, F2 the second, and so on. While the \
                     tooltip is up these keys go to PolterType alone, so they cannot reload \
                     a page or open an application's help.",
                ))
                .size(11)
                .color(b.muted)
            } else {
                unavailable(tr(
                    "suggestions.function_keys_unavailable",
                    "Not available on this system: PolterType cannot keep a key from the \
                     application here (Wayland, or macOS without [engine].hold_keys), and F5 \
                     would reload a browser page. The modifier + digit chord below stays in \
                     use.",
                ))
            });
        }
        if !function_keys_live {
            column = column.push(form_row(
                b,
                self.stacked(),
                tr(
                    "suggestions.keyboard_accept_modifiers",
                    "Keyboard accept modifiers",
                ),
                tr_args(
                    "suggestions.modifiers_hint",
                    "'+'-separated: {} — e.g. Ctrl+Shift. Applied with \
                     digit keys 1–9. Leave empty to disable keyboard accept.",
                    &[&named_key_list(&["Ctrl", "Shift", "Alt", "Meta"], ", ")],
                ),
                modifiers_input.into(),
            ));
            // Non-empty but chord-disabling input (bare `Shift`, a typo)
            // warns instead of being rejected: the engine treats it as "no
            // chord", so without this the setting looks configured while
            // doing nothing.
            if !s.accept_modifiers.trim().is_empty()
                && !accept_modifiers_enable_keyboard(&s.accept_modifiers)
            {
                column = column.push(
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
        }

        let nav_row = button_run(
            [
                (
                    SuggestionNavigation::Off,
                    tr("suggestions.navigation_off", "Off"),
                ),
                (
                    SuggestionNavigation::Arrows,
                    tr("suggestions.navigation_arrows", "Up/Down + Enter"),
                ),
            ]
            .into_iter()
            .map(|(choice, label)| {
                chip(
                    label,
                    navigation == choice,
                    live(Message::SuggestionNavigationChanged(choice)),
                )
            })
            .collect(),
        );
        let nav_note: Element<'_, Message> =
            if navigation == SuggestionNavigation::Arrows && !claimable {
                unavailable(tr(
                    "suggestions.navigation_unavailable",
                    "Not available on this system: PolterType cannot keep the arrow keys from \
                 the application here (Wayland, or macOS without [engine].hold_keys).",
                ))
                .into()
            } else {
                Text::new(tr(
                    "suggestions.navigation_hint",
                    "While the tooltip is up, Down highlights the first suggestion and Up the \
                 last; further presses move the highlight, Enter applies it and Esc closes \
                 the tooltip. Until the tooltip goes away, Up and Down do not move the text \
                 cursor, which is why this is off by default.",
                ))
                .size(11)
                .color(b.muted)
                .into()
            };
        column.push(
            Column::new()
                .spacing(6)
                .push(choice_block(
                    b,
                    tr("suggestions.navigation", "Arrow keys"),
                    "",
                    nav_row,
                ))
                .push(nav_note),
        )
    }
}
