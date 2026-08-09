//! The Setup pane: what still has to be granted, and how.
//!
//! Everything on screen comes from `poltertype_input::setup::probe`;
//! this file only decides how a [`SetupStep`] looks. "Is the user in
//! the `input` group" is platform code and belongs in the input crate,
//! while "what does an unresolved step look like" is a design question
//! and belongs here.
//!
//! Tone matters more than usual: this is the screen a user reaches when
//! the app they just installed does nothing, and the two failure modes
//! are equally bad — sounding alarmed about something that is fine, or
//! vague about something that is broken. So every row states what the
//! capability is *for*, and a step we cannot verify says so instead of
//! guessing.

use iced::widget::{Button, Column, Container, Row, Space, Text};
use iced::{Alignment, Element, Length, Padding};
use poltertype_core::i18n::tr;
use poltertype_input::setup::{StepAction, StepState};

use super::consts::PERMISSIONS_DOC_URL;
use super::enums::*;
use super::state::*;
use super::theme::{self, FONT_BOLD};
use super::view::{card, pane_header, section_title, status_line, tip};

impl SettingsApp {
    pub(super) fn view_setup(&self) -> Element<'_, Message> {
        let b = self.brand();
        let report = &self.setup;

        let headline = if report.needs_attention() {
            "PolterType needs one more permission before it can do anything."
        } else {
            "Everything PolterType needs is in place."
        };

        let mut steps = Column::new().spacing(14);
        for (i, step) in report.steps.iter().enumerate() {
            if i > 0 {
                steps = steps.push(Space::with_height(2));
            }
            steps = steps.push(self.step_row(step));
        }

        // The one sentence the `NeedsRelogin` state exists to say,
        // printed once rather than per step: every step in that state
        // has the same cause and the same fix, and repeating it makes
        // a two-row pane look like a wall of text.
        if report
            .steps
            .iter()
            .any(|s| s.state == StepState::NeedsRelogin)
        {
            steps = steps.push(Space::with_height(6));
            steps = steps.push(
                Text::new(
                    "Already set up — but this login session started before it, and a session \
                     keeps the group membership it was created with. Log out and back in (a \
                     reboot also does it). Re-running the setup script will not help.",
                )
                .size(12)
                .color(b.brand),
            );
        }

        let mut body = Column::new()
            .spacing(18)
            .push(pane_header(
                b,
                tr("setup.setup", "Setup"),
                headline.to_owned(),
            ))
            .push(card(steps));

        // The second failure mode, and a genuinely different one:
        // hooks fine, no way to change the layout. The app then
        // detects a wrong-layout word and corrects it *into the same
        // layout*, which looks like the correction being wrong rather
        // than missing. Only shown when it is actually true.
        if self.layout_backend.is_none() {
            body = body.push(card(
                Column::new()
                    .spacing(8)
                    .push(section_title(
                        b,
                        tr(
                            "setup.layout_switching_unavailable",
                            "Layout switching is unavailable",
                        ),
                    ))
                    .push(
                        Text::new(
                            "PolterType found no way to change the keyboard layout on this \
                             system. It can still detect a wrong-layout word and fix the \
                             letters, but the layout itself will not change — so the next \
                             word comes out wrong too.",
                        )
                        .size(12)
                        .color(b.muted),
                    )
                    .push(
                        Button::new(
                            Text::new(tr(
                                "setup.what_backends_are_supported",
                                "What backends are supported?",
                            ))
                            .size(12),
                        )
                        .on_press(Message::SetupOpen(PERMISSIONS_DOC_URL.to_owned()))
                        .style(theme::secondary)
                        .padding(button_padding()),
                    ),
            ));
        }

        let mut footer = Row::new().spacing(10).align_y(Alignment::Center).push(
            Button::new(Text::new(tr("setup.check_again", "Check again")).size(13))
                .on_press(Message::SetupRecheck)
                .style(theme::primary)
                .padding(Padding {
                    top: 7.0,
                    right: 16.0,
                    bottom: 7.0,
                    left: 16.0,
                }),
        );
        footer = footer.push(
            Button::new(Text::new(tr("setup.full_setup_guide", "Full setup guide")).size(12))
                .on_press(Message::SetupOpen(PERMISSIONS_DOC_URL.to_owned()))
                .style(theme::secondary)
                .padding(button_padding()),
        );
        if let Some(banner) = &self.setup_status {
            footer = footer.push(status_line(b, banner));
        }
        body = body.push(footer);

        // Changing a permission does not reach a process that already
        // started without it — on every OS. Saying so here is the
        // difference between "I granted it and nothing happened" and a
        // ten-second fix.
        body = body.push(tip(
            b,
            "Granted something just now? Quit PolterType from the tray and start it again — \
             permissions are read when the app starts, not while it runs.",
        ));

        if let Some(backend) = &self.setup.backend {
            body = body.push(tip(
                b,
                format!("Keyboard backend for this session: {backend}"),
            ));
        }

        body.into()
    }

    fn step_row(&self, step: &poltertype_input::setup::SetupStep) -> Element<'_, Message> {
        let b = self.brand();
        // A word, not a glyph. The bundled font renders ✓ and ↻ as
        // tofu boxes on this stack (× and → happen to survive), and a
        // status marker that shows as an empty rectangle is worse than
        // no marker at all. Words also say which of "not yet" and
        // "can't tell" this is, which a tick never could.
        let (mark, mark_color) = match step.state {
            StepState::Done => ("Ready", b.ecto),
            StepState::Todo => ("Needs you", b.garble),
            StepState::NeedsRelogin => ("Log out", b.brand),
            StepState::Unknown => ("Unknown", b.muted),
        };

        let mut text_col = Column::new()
            .spacing(4)
            .push(
                Text::new(step.title.clone())
                    .size(14)
                    .font(FONT_BOLD)
                    .color(b.ink),
            )
            .push(Text::new(step.detail.clone()).size(12).color(b.muted));

        if let Some(action) = &step.action {
            text_col = text_col.push(Space::with_height(2));
            text_col = text_col.push(action_button(action));
        }

        Row::new()
            .spacing(12)
            .align_y(Alignment::Start)
            .push(
                Container::new(Text::new(mark).size(11).font(FONT_BOLD).color(mark_color))
                    .width(74)
                    .padding(Padding {
                        top: 3.0,
                        right: 0.0,
                        bottom: 0.0,
                        left: 0.0,
                    }),
            )
            .push(text_col.width(Length::Fill))
            .into()
    }
}

/// One button per step, labelled by what it actually does. No step
/// ever offers to run something with `sudo` on the user's behalf —
/// `Copy` hands over the command instead, for them to read and run.
fn action_button(action: &StepAction) -> Element<'static, Message> {
    let (label, msg): (String, Message) = match action {
        StepAction::Open(url) if url.starts_with("x-apple") => (
            "Open System Settings".to_owned(),
            Message::SetupOpen(url.clone()),
        ),
        StepAction::Open(url) => ("Read the guide".to_owned(), Message::SetupOpen(url.clone())),
        StepAction::Copy(cmd) => (format!("Copy `{cmd}`"), Message::SetupCopy(cmd.clone())),
        StepAction::RequestPermission(p) => (
            "Ask macOS now".to_owned(),
            Message::SetupRequestPermission(*p),
        ),
    };
    Button::new(Text::new(label).size(12))
        .on_press(msg)
        .style(theme::secondary)
        .padding(button_padding())
        .into()
}

fn button_padding() -> Padding {
    Padding {
        top: 5.0,
        right: 12.0,
        bottom: 5.0,
        left: 12.0,
    }
}
