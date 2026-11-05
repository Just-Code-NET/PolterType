//! The About pane: who made this, where to reach them, and the one
//! irreversible button in the window.

use iced::widget::{Button, Column, Row, Space, Text};
use iced::{Alignment, Element, Font, Length, Padding};
use poltertype_core::i18n::{tr, tr_args};

use super::consts::{ISSUES_URL, JUST_CODE_URL, REPO_URL, SITE_URL};
use super::enums::*;
use super::state::*;
use super::theme::{self, font_bold};
use super::widgets::{action, button_run, card, divider, section_title};

impl SettingsApp {
    pub(super) fn view_about(&self) -> Element<'_, Message> {
        let b = self.brand();

        let hero = Column::new()
            .spacing(6)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .push(Space::new().height(8))
            .push(theme::mark(72))
            .push(Space::new().height(4))
            .push(
                Text::new("PolterType")
                    .size(26)
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
                .size(13)
                .color(b.ink)
                .align_x(Alignment::Center),
            )
            .push(Space::new().height(8))
            .push(
                Row::new()
                    .spacing(8)
                    .push(action("poltertype.com", Some(Message::OpenUrl(SITE_URL))))
                    .push(action("GitHub", Some(Message::OpenUrl(REPO_URL))))
                    .push(action(
                        tr("about.report_an_issue", "Report an issue"),
                        Some(Message::OpenUrl(ISSUES_URL)),
                    ))
                    .wrap()
                    .vertical_spacing(8)
                    .align_x(Alignment::Center),
            )
            .push(Space::new().height(10))
            .push(divider())
            .push(Space::new().height(6))
            .push(self.project_by())
            .push(Space::new().height(2));

        let maintenance = Column::new()
            .spacing(12)
            .push(section_title(
                b,
                tr(
                    "about.power_user_escape_hatches",
                    "Power-user escape hatches",
                ),
            ))
            .push(
                Text::new(tr_args(
                    "about.config_line",
                    "Config: {}",
                    &[&self.config_path.display().to_string()],
                ))
                .size(11)
                .font(Font::MONOSPACE)
                .color(b.muted),
            )
            .push(button_run(vec![
                action(
                    tr("general.open_config", "Open config.toml"),
                    Some(Message::OpenConfigFile),
                )
                .into(),
                action(
                    tr("about.reset_defaults", "Reset to defaults"),
                    Some(Message::ResetDefaults),
                )
                .style(theme::danger)
                .into(),
            ]))
            .push(
                Text::new(tr(
                    "about.reset_hint",
                    "Reset changes this window only; nothing is written until you press Save.",
                ))
                .size(11)
                .color(b.muted),
            );

        Column::new()
            .spacing(14)
            .push(card(hero))
            .push(card(maintenance))
            .into()
    }

    /// "A project by" and the maker's logo, which is the link.
    fn project_by(&self) -> Element<'_, Message> {
        let b = self.brand();
        Row::new()
            .spacing(10)
            .align_y(Alignment::Center)
            .push(
                Text::new(tr("about.project_by", "A project by"))
                    .size(13)
                    .color(b.muted),
            )
            .push(
                Button::new(theme::just_code_logo(self.is_dark()))
                    .style(theme::nav(false))
                    .padding(Padding {
                        top: 4.0,
                        right: 8.0,
                        bottom: 4.0,
                        left: 8.0,
                    })
                    .on_press(Message::OpenUrl(JUST_CODE_URL)),
            )
            .into()
    }
}
