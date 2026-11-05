//! The General pane: how the app behaves, how it looks, how it updates.

use iced::widget::{Column, Text};
use iced::{Element, Font};
use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::settings::TrayIconStyle;
use poltertype_input::ReplaySpeed;

use super::enums::*;
use super::state::*;
use super::widgets::{
    action, button_run, card, chip, choice_block, form_row, pane_header, section_title, stepper,
    switch,
};

impl SettingsApp {
    pub(super) fn view_general(&self) -> Element<'_, Message> {
        let b = self.brand();
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
            .push(card(self.general_behaviour()))
            .push(card(self.general_timing()))
            .push(card(self.general_appearance()))
            .push(card(self.general_updates()))
            .push(card(self.general_folders()))
            .into()
    }

    fn general_behaviour(&self) -> Column<'_, Message> {
        let b = self.brand();
        let g = &self.settings.general;
        let e = &self.settings.engine;

        // The same flag the tray's pause item writes, named as the mode
        // it is: the request behind it was for manual-only conversion,
        // from someone who had found the pause and read it as the app
        // being off (issue #51).
        let conversion = button_run(
            [
                (false, tr("general.conversion_auto", "Automatic")),
                (true, tr("general.conversion_manual", "Manual only")),
            ]
            .into_iter()
            .map(|(manual, label)| {
                chip(
                    label,
                    g.paused == manual,
                    Some(Message::ManualOnlyChosen(manual)),
                )
            })
            .collect(),
        );

        Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.behaviour", "Behaviour")))
            .push(choice_block(
                b,
                tr("general.conversion", "Conversion"),
                tr(
                    "general.conversion_hint",
                    "Manual only watches what you type but corrects nothing on its own — \
                     the last word is converted when you press the manual hotkey. Same \
                     switch as the tray's Pause auto-switch, so the tray icon shows it too.",
                ),
                conversion,
            ))
            .push(switch(
                g.autostart,
                tr(
                    "general.start_automatically_when_i",
                    "Start automatically when I sign in",
                ),
                Message::AutostartToggled,
            ))
            .push(switch(
                g.sound_on_correct,
                tr(
                    "general.play_soft_chime_on",
                    "Play a soft chime on correction",
                ),
                Message::SoundOnCorrectToggled,
            ))
            .push(switch(
                g.show_notifications,
                tr(
                    "general.show_second_system_notification",
                    "Show a 2-second system notification on auto-switch",
                ),
                Message::ShowNotificationsToggled,
            ))
            .push(switch(
                e.suppress_in_identifiers,
                tr(
                    "general.skip_auto_switch_on",
                    "Skip auto-switch on identifiers (foo_bar, snake_case, …)",
                ),
                Message::SuppressInIdentifiersToggled,
            ))
    }

    fn general_timing(&self) -> Column<'_, Message> {
        let b = self.brand();
        let e = &self.settings.engine;
        let speed_choice = ReplaySpeed::from_config(&e.replay_speed);
        let speed = button_run(
            [
                (ReplaySpeed::Normal, tr("general.speed_normal", "Normal")),
                (ReplaySpeed::Fast, tr("general.speed_fast", "Fast")),
                (ReplaySpeed::Instant, tr("general.speed_instant", "Instant")),
            ]
            .into_iter()
            .map(|(choice, label)| {
                chip(
                    label,
                    speed_choice == choice,
                    Some(Message::ReplaySpeedChanged(choice)),
                )
            })
            .collect(),
        );

        Column::new()
            .spacing(14)
            .push(section_title(b, tr("general.timing", "Correction timing")))
            .push(choice_block(
                b,
                tr("general.replay_speed", "Replacement speed"),
                tr(
                    "general.replay_speed_hint",
                    "How quickly a correction happens — both the pauses between \
                     keystrokes and the waits around the layout switch. Normal is paced \
                     for input remappers such as keyd, which drop keystrokes sent \
                     faster; try a faster setting and go back if letters start going \
                     missing. Applies to the next correction.",
                ),
                speed,
            ))
            .push(form_row(
                b,
                self.stacked(),
                tr("general.idle_timeout_ms", "Idle timeout (ms)"),
                tr(
                    "general.buffer_cleared_after_this",
                    "Buffer is cleared after this much keyboard silence.",
                ),
                stepper(
                    true,
                    e.idle_timeout_ms.to_string(),
                    b.ink,
                    Message::IdleTimeoutDelta(-100),
                    Message::IdleTimeoutDelta(100),
                ),
            ))
    }

    fn general_appearance(&self) -> Column<'_, Message> {
        let b = self.brand();

        // Applies instantly; persisted by the footer Save.
        let theme_row = button_run(
            ThemeChoice::ALL
                .into_iter()
                .map(|choice| {
                    chip(
                        choice.label(),
                        self.theme_choice() == choice,
                        Some(Message::ThemeChoiceChanged(choice)),
                    )
                })
                .collect(),
        );
        // Takes effect where the tray reads it, so unlike the theme
        // above there is nothing to preview here.
        let tray_choice = TrayIconStyle::from_config(&self.settings.general.tray_icon);
        let tray_row = button_run(
            [
                (TrayIconStyle::Color, tr("general.tray_color", "Colour")),
                (TrayIconStyle::Flag, tr("general.tray_flag", "Flag")),
                (TrayIconStyle::Mono, tr("general.tray_mono", "Mono")),
                (TrayIconStyle::Hidden, tr("general.tray_hidden", "Hidden")),
            ]
            .into_iter()
            .map(|(choice, label)| {
                chip(
                    label,
                    tray_choice == choice,
                    Some(Message::TrayIconChoiceChanged(choice)),
                )
            })
            .collect(),
        );

        let mut appearance = Column::new()
            .spacing(14)
            .push(section_title(b, tr("general.appearance", "Appearance")))
            .push(choice_block(
                b,
                tr("general.interface_language", "Interface language"),
                tr(
                    "general.interface_language_hint",
                    "Applies as you pick it; the tray menu follows once you save. Anything \
                     not translated stays English, and a plug-in can bring a language of \
                     its own.",
                ),
                self.language_chips(),
            ));
        if let Some(trouble) = language_trouble() {
            appearance = appearance.push(Text::new(trouble).size(11).color(b.warn));
        }
        appearance
            .push(choice_block(
                b,
                tr("general.theme", "Theme"),
                tr(
                    "general.system_follows_os_light",
                    "System follows the OS light/dark preference. Save to persist.",
                ),
                theme_row,
            ))
            .push(choice_block(
                b,
                tr("general.tray_icon", "Tray icon"),
                tr(
                    "general.tray_icon_hint",
                    "Colour gives every layout its own; Flag draws its country, and \
                     falls back to Colour where there is no drawing for one; Mono \
                     keeps one neutral badge. Hidden removes the icon and its menu — \
                     reopen this window by running poltertype --settings.",
                ),
                tray_row,
            ))
    }

    /// Offers what is on disk, not what was compiled in: a translation
    /// dropped into the config directory — or one a plug-in brought with
    /// it — is selectable without anybody editing `config.toml` by hand.
    fn language_chips(&self) -> Element<'_, Message> {
        let current = self.settings.general.ui_language.trim();
        let automatic = current.is_empty()
            || current.eq_ignore_ascii_case("system")
            || current.eq_ignore_ascii_case("auto");
        let mut chips = vec![chip(
            tr("general.language_system", "System"),
            automatic,
            Some(Message::UiLanguageChanged("system".to_owned())),
        )];
        chips.extend(self.ui_languages.iter().map(|(code, name)| {
            chip(
                language_label(code, name),
                !automatic && code.eq_ignore_ascii_case(current),
                Some(Message::UiLanguageChanged(code.clone())),
            )
        }));
        // A code with no catalog on this machine is still what the file
        // says, so it is shown rather than silently replaced by "System".
        let known = self
            .ui_languages
            .iter()
            .any(|(code, _)| code.eq_ignore_ascii_case(current));
        if !automatic && !known {
            chips.push(chip(current.to_owned(), true, None));
        }
        button_run(chips)
    }

    /// The app's disclosure surface for the one thing it does that
    /// reaches outside the machine, so it names the exact URL rather than
    /// saying "checks for updates" and leaving the user to trust us.
    fn general_updates(&self) -> Column<'_, Message> {
        let b = self.brand();
        let u = &self.settings.updates;
        let mut updates = Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.updates", "Updates")))
            .push(switch(
                u.enabled,
                tr(
                    "general.download_new_versions_automatically",
                    "Download new versions automatically, install on restart",
                ),
                Message::AutoUpdateToggled,
            ))
            .push(form_row(
                b,
                self.stacked(),
                tr("general.check_every_hours", "Check every (hours)"),
                "",
                // With checking off the interval means nothing, and the
                // disabled buttons are exactly that signal.
                stepper(
                    u.enabled,
                    u.check_interval_hours.to_string(),
                    if u.enabled { b.ink } else { b.muted },
                    Message::UpdateIntervalDelta(-1),
                    Message::UpdateIntervalDelta(1),
                ),
            ))
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
        updates
    }

    fn general_folders(&self) -> Column<'_, Message> {
        let b = self.brand();
        Column::new()
            .spacing(12)
            .push(section_title(b, tr("general.folders", "Folders")))
            .push(button_run(vec![
                action(
                    tr("general.open_config", "Open config.toml"),
                    Some(Message::OpenConfigFile),
                )
                .into(),
                action(tr("general.open_logs", "Logs"), Some(Message::OpenLogsDir)).into(),
                action(
                    tr("general.open_wordlists", "User wordlists"),
                    Some(Message::OpenWordlistsDir),
                )
                .into(),
                action(
                    tr("general.open_layouts", "User layouts"),
                    Some(Message::OpenLayoutsDir),
                )
                .into(),
            ]))
    }
}

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

/// What to say under the picker when the catalogs did not come out the
/// way the files promised.
///
/// Silence is the normal answer, and the one this returns for a
/// language that simply has no file: that is a translation nobody
/// wrote, not one that failed. The two cases worth a line are a file
/// that was found and could not be fully read, and a language that was
/// asked for and produced nothing — because from the outside those
/// look identical to a translation that works, and the only account of
/// either was a log line nobody has a reason to read (#64).
fn language_trouble() -> Option<String> {
    let catalog = poltertype_core::i18n::active_catalog()?;
    if let Some(problem) = catalog.problems().first() {
        return Some(tr_args(
            "general.language_partly_read",
            "Part of this translation could not be read — {}",
            &[problem],
        ));
    }
    (catalog.is_empty() && !catalog.locale().starts_with("en")).then(|| {
        tr_args(
            "general.language_not_found",
            "No catalog found for {}, so the interface stays in English.",
            &[catalog.locale()],
        )
    })
}
