//! Message handling: the `update` half of the iced loop.

use iced::Task;
use iced::widget::text_editor;
use poltertype_core::plugins::SettingValue;
use poltertype_core::settings::{MIN_UPDATE_INTERVAL_HOURS, Settings, SettingsStore};
use tracing::{info, warn};

use super::enums::*;
use super::helpers::*;
use super::plugin_pane::ReportState;
use super::state::*;

impl SettingsApp {
    pub(super) fn update(&mut self, msg: Message) -> Task<Message> {
        // Any user-visible edit clears the previous banner — keeps
        // the footer accurate (otherwise "Saved!" sticks around even
        // as the user starts editing again).
        if !matches!(msg, Message::Save | Message::Reload) {
            self.save_banner = None;
        }

        match msg {
            Message::SelectPane(p) => {
                self.pane = p;
                // Reports are asked for on the way into the pane, not
                // on every draw: each one costs a process.
                if p == Pane::Plugins {
                    return self.load_pending_reports();
                }
            }

            // Every plug-in edit writes straight through to the
            // plug-in's own file: it may be running and watching that
            // file, so a pane holding changes back would be showing a
            // state the plug-in is not in.
            Message::PluginToggled(plugin, index, on) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set(index, SettingValue::Bool(on));
                }
            }
            Message::PluginChoiceSelected(plugin, index, chosen) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set(index, SettingValue::Text(chosen));
                }
            }
            Message::PluginTextChanged(plugin, index, text) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    // A number control stores a number; anything that
                    // is not one yet is left in the box rather than
                    // written, so a half-typed "12" does not land as 1.
                    let value = match pane.control(index).map(|c| c.kind) {
                        Some(poltertype_core::plugins::ControlKind::Number) => {
                            match text.trim().parse::<i64>() {
                                Ok(n) => Some(SettingValue::Int(n)),
                                Err(_) => None,
                            }
                        }
                        _ => Some(SettingValue::Text(text)),
                    };
                    if let Some(value) = value {
                        pane.set(index, value);
                    }
                }
            }
            Message::PluginReportRefresh(plugin, control) => {
                return self.load_report(plugin, control);
            }
            Message::PluginReportLoaded(plugin, control, outcome) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.reports.insert(
                        control,
                        match outcome {
                            Ok(text) => ReportState::Ready(text),
                            Err(why) => ReportState::Failed(why),
                        },
                    );
                }
            }
            Message::PluginCommandClicked(plugin, command) => {
                if let Some(pane) = self.plugins.get(plugin) {
                    if let Err(e) = crate::plugins::run_command(&pane.ext, &command) {
                        tracing::warn!("plug-in button failed: {e}");
                    }
                }
            }

            Message::LanguageToggled(id, active) => {
                // The "Active" checkbox renders the *effective* state,
                // not the raw `[languages].active` list — empty list
                // means "consider every OS layout", so all checkboxes
                // start ticked. When the user unticks one of them in
                // that implicit-all mode, we materialise the list as
                // "every OS layout EXCEPT this one" so the user's
                // intent ("don't use this one") survives a save.
                //
                // The opposite — re-ticking the same box — appends it
                // back. We don't auto-collapse the list back to empty
                // even if it ends up containing every OS-active layout
                // again, because the effective behaviour is identical
                // and a future OS-layout add should still be honoured.
                let list = &mut self.settings.languages.active;
                let was_implicit_all = list.is_empty();
                if active {
                    if !list.contains(&id) {
                        list.push(id);
                    }
                } else if was_implicit_all {
                    *list = self
                        .os_layouts
                        .iter()
                        .filter(|l| **l != id)
                        .cloned()
                        .collect();
                } else {
                    list.retain(|x| *x != id);
                }
            }
            Message::LanguageIgnoreToggled(id, ignored) => {
                let list = &mut self.settings.languages.ignored;
                if ignored {
                    if !list.contains(&id) {
                        list.push(id);
                    }
                } else {
                    list.retain(|x| *x != id);
                }
            }
            Message::AutostartToggled(b) => self.settings.general.autostart = b,
            Message::SoundOnCorrectToggled(b) => self.settings.general.sound_on_correct = b,
            Message::ShowNotificationsToggled(b) => self.settings.general.show_notifications = b,
            Message::SuppressInIdentifiersToggled(b) => {
                self.settings.engine.suppress_in_identifiers = b
            }
            Message::IdleTimeoutDelta(delta) => {
                let cur = i32::try_from(self.settings.engine.idle_timeout_ms).unwrap_or(2000);
                let next = (cur + delta).clamp(250, 60_000);
                self.settings.engine.idle_timeout_ms = u64::try_from(next).unwrap_or(2000);
            }
            Message::AutoUpdateToggled(b) => self.settings.updates.enabled = b,
            Message::UpdateIntervalDelta(delta) => {
                // Floor is the same `MIN_UPDATE_INTERVAL_HOURS` the
                // engine clamps to at read time — the UI must not be
                // able to express a value the app would silently ignore.
                // Ceiling is a week: beyond that "automatic updates" is
                // a checkbox that lies.
                let cur = i64::try_from(self.settings.updates.check_interval_hours).unwrap_or(24);
                let floor = i64::try_from(MIN_UPDATE_INTERVAL_HOURS).unwrap_or(1);
                let next = (cur + delta).clamp(floor, 24 * 7);
                self.settings.updates.check_interval_hours = u64::try_from(next).unwrap_or(24);
            }

            // ── Hotkeys ──────────────────────────────────────────
            Message::HotkeyRebindStart(kind) => self.capturing = Some(kind),
            Message::HotkeyRebindCancel => self.capturing = None,
            Message::HotkeyCaptured(combo) => {
                if let Some(kind) = self.capturing.take() {
                    info!(?kind, %combo, "captured new hotkey combo");
                    match kind {
                        HotkeyKind::Pause => {
                            self.settings.hotkeys.pause_toggle = combo;
                        }
                        HotkeyKind::SwitchLast => {
                            self.settings.hotkeys.manual_switch_last = combo;
                        }
                    }
                }
            }

            // ── Exceptions ───────────────────────────────────────
            Message::ExceptionDraftChanged(s) => self.exception_draft = s,
            Message::ExceptionAdd => {
                let trimmed = self.exception_draft.trim().to_owned();
                if !trimmed.is_empty()
                    && !self
                        .settings
                        .exceptions
                        .disabled_apps
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&trimmed))
                {
                    self.settings.exceptions.disabled_apps.push(trimmed);
                }
                self.exception_draft.clear();
            }
            Message::ExceptionRemove(idx) => {
                if idx < self.settings.exceptions.disabled_apps.len() {
                    self.settings.exceptions.disabled_apps.remove(idx);
                }
            }

            // ── Commands ────────────────────────────────────────
            Message::CommandDraftNameChanged(s) => self.command_draft_name = s,
            Message::CommandDraftTriggerChanged(s) => self.command_draft_trigger = s,
            Message::CommandDraftActionKindChanged(kind) => {
                if self.command_draft_action_kind != kind {
                    // Different action variants take wildly different
                    // content (snippet vs layout id vs URL); flipping
                    // the radio without clearing the field would leave
                    // a confusing half-typed value behind.
                    self.command_draft_param.clear();
                }
                self.command_draft_action_kind = kind;
            }
            Message::CommandDraftParamChanged(s) => self.command_draft_param = s,
            Message::CommandDraftAppsChanged(s) => self.command_draft_apps = s,
            Message::CommandAdd => match build_command_from_draft(self) {
                Ok(cmd) => {
                    info!(id = %cmd.id, "adding user command from UI");
                    self.settings.commands.push(cmd);
                    // Clear the draft on success.
                    self.command_draft_name.clear();
                    self.command_draft_trigger.clear();
                    self.command_draft_param.clear();
                    self.command_draft_apps.clear();
                    self.command_status = Some(SaveBanner {
                        text: "Added. Press Save to persist, then restart poltertype.".into(),
                        is_error: false,
                    });
                }
                Err(e) => {
                    self.command_status = Some(SaveBanner {
                        text: e,
                        is_error: true,
                    });
                }
            },
            Message::CommandRemove(idx) => {
                if idx < self.settings.commands.len() {
                    let removed = self.settings.commands.remove(idx);
                    info!(id = %removed.id, "removed user command from UI");
                    self.command_status = Some(SaveBanner {
                        text: format!("Removed `{}`.", removed.id),
                        is_error: false,
                    });
                }
            }

            // ── Wordlists ────────────────────────────────────────
            //
            // All three selectors below auto-flush the editor to
            // disk before switching context. Without this, a user
            // who typed words and clicked another layout/profile/kind
            // button to "see what's there" would silently lose the
            // unsaved content — the next handler unconditionally
            // overwrites the buffer with the freshly-loaded file.
            // Flushing first is friendlier than an unsaved-changes
            // dialog and matches what most editors do on file
            // switch.
            Message::WordlistProfileSelected(profile_id) => {
                let outcome = self.flush_wordlist_to_disk();
                self.wordlist_profile = profile_id;
                if let Some(id) = self.wordlist_layout.clone() {
                    let text =
                        read_overlay_file_or_empty(&self.wordlist_profile, &id, self.wordlist_kind);
                    self.wordlist_content = text_editor::Content::with_text(&text);
                    self.wordlist_dirty = false;
                    self.wordlist_status = banner_for_auto_save(outcome);
                }
            }
            Message::WordlistLayoutSelected(id) => {
                let outcome = self.flush_wordlist_to_disk();
                self.wordlist_layout = Some(id.clone());
                let text =
                    read_overlay_file_or_empty(&self.wordlist_profile, &id, self.wordlist_kind);
                self.wordlist_content = text_editor::Content::with_text(&text);
                self.wordlist_dirty = false;
                self.wordlist_status = banner_for_auto_save(outcome);
            }
            Message::WordlistKindSelected(kind) => {
                let outcome = self.flush_wordlist_to_disk();
                self.wordlist_kind = kind;
                if let Some(id) = &self.wordlist_layout {
                    let text = read_overlay_file_or_empty(&self.wordlist_profile, id, kind);
                    self.wordlist_content = text_editor::Content::with_text(&text);
                    self.wordlist_dirty = false;
                    self.wordlist_status = banner_for_auto_save(outcome);
                }
            }
            Message::WordlistEdit(action) => {
                // `Action::is_edit()` flips the dirty flag only on
                // semantic edits (insert / delete / paste). Cursor
                // moves and scroll events leave it false so we don't
                // ask the user to save a buffer they only looked at.
                if action.is_edit() {
                    self.wordlist_dirty = true;
                }
                self.wordlist_content.perform(action);
            }
            // ── Suggestions ──────────────────────────────────────
            Message::SuggestionsToggled(b) => self.settings.suggestions.enabled = b,
            Message::SuggestionMaxDelta(delta) => {
                // 1..=9 is the same clamp `SuggestionSettings::
                // max_clamped` applies at read time — each entry is
                // addressed by one digit key, so the UI must not be
                // able to express a count the engine would ignore.
                let cur = i64::try_from(self.settings.suggestions.max_suggestions).unwrap_or(5);
                let next = (cur + delta).clamp(1, 9);
                self.settings.suggestions.max_suggestions = usize::try_from(next).unwrap_or(5);
            }
            Message::SuggestionTimeoutDelta(delta) => {
                // Same 3..=600 window `SuggestionSettings::timeout`
                // clamps to at read time — see the update-interval
                // rationale above for why the UI mirrors the clamp.
                let cur =
                    i64::try_from(self.settings.suggestions.tooltip_timeout_secs).unwrap_or(30);
                let next = (cur + delta).clamp(3, 600);
                self.settings.suggestions.tooltip_timeout_secs = u64::try_from(next).unwrap_or(30);
            }
            Message::SuggestionModifiersChanged(s) => {
                self.settings.suggestions.accept_modifiers = s
            }

            Message::ThemeChoiceChanged(choice) => {
                self.settings.general.ui_theme = choice.config_value().to_owned();
            }

            Message::ResetDefaults => self.settings = Settings::default(),
            Message::Reload => match SettingsStore::load_or_default() {
                Ok(fresh) => {
                    self.settings = fresh.snapshot();
                    // Also re-read the current wordlist file into the
                    // editor — keeps footer Reload's contract uniform:
                    // "reset every on-disk-backed view to what's on
                    // disk right now". Discards unsaved editor content
                    // by design, just like the old per-pane Reload
                    // button did. Auto-save on layout/profile/kind
                    // switch usually means there's nothing unsaved to
                    // lose here anyway.
                    if let Some(id) = self.wordlist_layout.clone() {
                        let text = read_overlay_file_or_empty(
                            &self.wordlist_profile,
                            &id,
                            self.wordlist_kind,
                        );
                        self.wordlist_content = text_editor::Content::with_text(&text);
                        self.wordlist_dirty = false;
                        self.wordlist_status = None;
                    }
                    self.save_banner = Some(SaveBanner {
                        text: "Reloaded from disk.".into(),
                        is_error: false,
                    });
                }
                Err(e) => {
                    self.save_banner = Some(SaveBanner {
                        text: format!("Reload failed: {e}"),
                        is_error: true,
                    });
                }
            },
            Message::Save => {
                // Footer "Save" saves EVERYTHING — config.toml AND
                // any unsaved edits in the Wordlists pane. Without
                // this, a user who typed a word, hit the prominent
                // footer Save (more visually weighted than the
                // per-pane Save), and closed the window would lose
                // their wordlist edit silently — exactly the bug
                // report that prompted this fix.
                //
                // We flush the wordlist FIRST so the pane's own
                // banner reflects what happened (per-pane state
                // wins), then save config.toml and update the
                // global save banner.
                let wordlist_outcome = self.flush_wordlist_to_disk();
                if !matches!(wordlist_outcome, WordlistFlushOutcome::Nothing) {
                    self.wordlist_status = Some(banner_for_wordlist_save(wordlist_outcome));
                }
                let staged = self.settings.clone();
                match self.store.update(|s| *s = staged) {
                    Ok(()) => {
                        info!(path = ?self.config_path, "settings saved from UI");
                        self.save_banner = Some(SaveBanner {
                            text: format!("Saved to {}.", self.config_path.display()),
                            is_error: false,
                        });
                    }
                    Err(e) => {
                        warn!(?e, "settings save failed");
                        self.save_banner = Some(SaveBanner {
                            text: format!("Save failed: {e}"),
                            is_error: true,
                        });
                    }
                }
            }

            Message::OpenConfigFile => {
                let _ = opener::open(&self.config_path);
            }
            Message::OpenLogsDir => {
                if let Ok(dir) = SettingsStore::log_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = opener::open(&dir);
                }
            }
            Message::OpenWordlistsDir => {
                if let Some(dir) = poltertype_core::layouts::user_wordlist_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = opener::open(&dir);
                }
            }
            Message::OpenLayoutsDir => {
                if let Some(dir) = poltertype_core::layouts::user_layout_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = opener::open(&dir);
                }
            }
            Message::OpenUrl(url) => {
                // `opener` routes http(s) URLs to the default browser.
                let _ = opener::open(url);
            }

            // ── Setup pane ─────────────────────────────────────────
            Message::SetupRecheck => {
                let before = self.setup.clone();
                self.setup = poltertype_input::setup::probe_setup();
                // Say something either way. A button that silently
                // redraws the same screen reads as broken, and "still
                // not granted" is the answer a user in the middle of
                // fixing permissions most needs to hear.
                self.setup_status = Some(if self.setup == before {
                    SaveBanner {
                        text: if self.setup.needs_attention() {
                            "Checked — nothing has changed yet.".to_owned()
                        } else {
                            "Checked — everything is in place.".to_owned()
                        },
                        is_error: false,
                    }
                } else {
                    SaveBanner {
                        text: "Checked — something changed. Restart PolterType to pick it up."
                            .to_owned(),
                        is_error: false,
                    }
                });
            }
            Message::SetupOpen(url) => {
                // Covers http(s) documentation links and macOS
                // `x-apple.systempreferences:` deep links alike —
                // `opener` hands both to the OS handler.
                if let Err(e) = opener::open(&url) {
                    warn!(?e, %url, "could not open setup link");
                    self.setup_status = Some(SaveBanner {
                        text: format!("Couldn't open {url}"),
                        is_error: true,
                    });
                }
            }
            Message::SetupCopy(command) => {
                self.setup_status = Some(SaveBanner {
                    text: format!("Copied: {command}"),
                    is_error: false,
                });
                return iced::clipboard::write(command);
            }
            Message::SetupRequestPermission(permission) => {
                // The OS shows its own dialog; ours never imitates
                // one. Accessibility's prompt is asynchronous, so the
                // return value is not an answer — re-probe instead of
                // believing it.
                poltertype_input::setup::request_permission(permission);
                self.setup = poltertype_input::setup::probe_setup();
                self.setup_status = Some(SaveBanner {
                    text: "Asked the system. Approve it there, then press Check again.".to_owned(),
                    is_error: false,
                });
            }

            Message::WindowCloseRequested(id) => {
                // Last chance to flush any unsaved wordlist edit
                // before the window goes away. Failures are logged
                // (already done inside flush) but don't block the
                // close — leaving a window the user explicitly
                // asked to close in some half-closed state would be
                // worse than losing one save.
                let _ = self.flush_wordlist_to_disk();
                return iced::window::close(id);
            }
        }
        Task::none()
    }

    /// Write the current wordlist editor buffer to its resolved
    /// overlay file. Returns an outcome describing what happened so
    /// the caller can pick the right banner phrasing.
    ///
    /// This is the single shared "save the wordlist now" path,
    /// called by:
    ///
    /// * `Message::WordlistSave` — explicit per-pane Save click.
    /// * `Message::Save` — footer Save click (must save everything,
    ///   not just `config.toml`).
    /// * `Message::WordlistProfileSelected` /
    ///   `WordlistLayoutSelected` / `WordlistKindSelected` —
    ///   auto-save before switching context, so a user who typed
    ///   words and toggled to "see another layout" doesn't lose
    ///   their edit.
    ///
    /// On success, clears the dirty flag. Doesn't touch
    /// `wordlist_status` — the caller picks the banner text via
    /// `banner_for_wordlist_save` / `banner_for_auto_save` so the
    /// phrasing matches the trigger ("Saved." vs "Auto-saved.").
    /// Ask for every report on this pane that has not been asked yet.
    pub(super) fn load_pending_reports(&mut self) -> Task<Message> {
        let wanted: Vec<(usize, usize)> = self
            .plugins
            .iter()
            .enumerate()
            .flat_map(|(plugin, pane)| {
                pane.unasked_reports()
                    .into_iter()
                    .map(move |control| (plugin, control))
            })
            .collect();
        Task::batch(
            wanted
                .into_iter()
                .map(|(plugin, control)| self.load_report(plugin, control))
                .collect::<Vec<_>>(),
        )
    }

    /// Run one report command off the UI thread and deliver the answer
    /// as a message.
    ///
    /// A plain thread rather than anything cleverer: the work is one
    /// blocking wait on a child process, the runtime underneath iced is
    /// not ours to assume, and a oneshot channel bridges the two
    /// without either of them having to know about the other.
    pub(super) fn load_report(&mut self, plugin: usize, control: usize) -> Task<Message> {
        let Some(pane) = self.plugins.get_mut(plugin) else {
            return Task::none();
        };
        let Some(declared) = pane.ext.manifest.pane.get(control) else {
            return Task::none();
        };
        let (ext, command) = (pane.ext.clone(), declared.command.clone());
        pane.reports.insert(control, ReportState::Loading);

        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::plugins::read_report(&ext, &command));
        });
        Task::perform(
            async move {
                rx.await
                    .unwrap_or_else(|_| Err("the report task went away".to_owned()))
            },
            move |outcome| Message::PluginReportLoaded(plugin, control, outcome),
        )
    }

    pub(super) fn flush_wordlist_to_disk(&mut self) -> WordlistFlushOutcome {
        if !self.wordlist_dirty {
            return WordlistFlushOutcome::Nothing;
        }
        let Some(id) = self.wordlist_layout.clone() else {
            return WordlistFlushOutcome::NoLayout;
        };
        let text = self.wordlist_content.text();
        match save_overlay_file(&self.wordlist_profile, &id, self.wordlist_kind, &text) {
            Ok(path) => {
                info!(
                    path = ?path,
                    layout = %id,
                    kind = ?self.wordlist_kind,
                    profile = %self.wordlist_profile,
                    "wordlist flushed to disk"
                );
                self.wordlist_dirty = false;
                WordlistFlushOutcome::Saved(path)
            }
            Err(e) => {
                warn!(
                    layout = %id,
                    kind = ?self.wordlist_kind,
                    profile = %self.wordlist_profile,
                    err = %e,
                    "wordlist flush failed"
                );
                WordlistFlushOutcome::Failed(e.to_string())
            }
        }
    }
}
