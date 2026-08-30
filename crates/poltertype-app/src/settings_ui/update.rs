//! Message handling: the `update` half of the iced loop.

use iced::Task;
use iced::widget::text_editor;
use poltertype_core::plugins::SettingValue;
use poltertype_core::settings::{MIN_UPDATE_INTERVAL_HOURS, Settings, SettingsStore};
use tracing::{info, warn};

use super::enums::*;
use super::helpers::*;
use super::plugin_pane::{CommandOutput, Slot, Typing};
use super::state::*;

impl SettingsApp {
    pub(super) fn update(&mut self, msg: Message) -> Task<Message> {
        // Any user-visible edit clears the previous banner, so "Saved!"
        // cannot linger while the user is editing again.
        if !matches!(msg, Message::Save | Message::Reload) {
            self.save_banner = None;
        }

        // Anything other than typing in the same box settles what was
        // typed into a plug-in's box — closing the window included,
        // which is why this sits above the match and not in each arm.
        // See [`PluginPane::flush_edits`].
        let typing = match &msg {
            Message::PluginTextChanged(plugin, control, _) => {
                Some((*plugin, Typing::Control(*control)))
            }
            Message::PluginRecordTyped(plugin, control, row, field, _) => Some((
                *plugin,
                Typing::Record {
                    control: *control,
                    row: *row,
                    field: field.clone(),
                },
            )),
            _ => None,
        };
        for (i, pane) in self.plugins.iter_mut().enumerate() {
            let held = typing.as_ref().filter(|(p, _)| *p == i).map(|(_, t)| t);
            pane.flush_edits(held);
        }

        match msg {
            Message::SelectPane(p) => {
                self.pane = p;
                // Reports are asked for on the way into the pane, not
                // on every draw: each one costs a process.
                if p == Pane::Plugins {
                    return self.load_pending_outputs();
                }
            }

            // Every plug-in edit writes straight through to the
            // plug-in's own file: it may be running and watching that
            // file, so held-back changes would show a state the plug-in
            // is not in.
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
                    pane.set_text(index, text);
                }
            }
            Message::PluginRecordChanged(plugin, index, row, field, value) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set_record(index, row, &field, value);
                }
            }
            Message::PluginRecordTyped(plugin, index, row, field, text) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set_record_text(index, row, &field, text);
                }
            }
            Message::PluginRecordAdded(plugin, index) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    // Anything half-typed settles first: adding a row
                    // rewrites the file, and a flush afterwards would
                    // write yesterday's text against the new numbering.
                    pane.flush_edits(None);
                    pane.add_record(index);
                }
            }
            Message::PluginRecordRemoved(plugin, index, row) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.flush_edits(None);
                    pane.remove_record(index, row);
                }
            }
            // Everything typed settles first: the plug-in reads its
            // config file to find out what it was asked to do, so a
            // message still in a text box is one it will not send.
            Message::PluginRecordAction(plugin, index, row, command) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.flush_edits(None);
                }
                let Some(pane) = self.plugins.get_mut(plugin) else {
                    return Task::none();
                };
                // Nothing to run it against: the field the manifest
                // named as the row's identity is empty.
                let Some(id) = pane.record_id(index, row) else {
                    pane.status = Some(
                        "This one has no name yet — give it one before acting on it.".to_owned(),
                    );
                    return Task::none();
                };
                pane.set_action_running(Some((index, row)));
                pane.status = Some(format!("Running “{id}”…"));
                let ext = pane.ext.clone();

                // Waited on off the UI thread: the action behind such a
                // button steals focus, drives somebody else's chat
                // client and types a sentence — seconds, not
                // milliseconds.
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                let handed = id.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::plugins::run_command_for_row_waiting(
                        &ext, &command, &handed,
                    ));
                });
                return Task::perform(
                    async move {
                        rx.await
                            .unwrap_or_else(|_| Err("the action task went away".to_owned()))
                    },
                    move |outcome| Message::PluginRecordActionDone(plugin, id.clone(), outcome),
                );
            }
            Message::PluginRecordActionDone(plugin, id, outcome) => {
                let refresh = {
                    let Some(pane) = self.plugins.get_mut(plugin) else {
                        return Task::none();
                    };
                    pane.set_action_running(None);
                    pane.status = Some(match &outcome {
                        // The plug-in's own words: it is the only thing
                        // here that knows whether the message went.
                        Ok(text) if !text.trim().is_empty() => first_lines(text, 3),
                        Ok(_) => format!("“{id}” finished without saying anything."),
                        // The plug-in usually names the row itself —
                        // saying it twice reads as two different things
                        // having gone wrong.
                        Err(why) if why.contains(id.as_str()) => why.clone(),
                        Err(why) => format!("“{id}”: {why}"),
                    });
                    // The reports on this group are out of date — the
                    // action just changed them. Only the reports:
                    // re-asking a conversation list would read a chat
                    // client's sidebar for an unrelated button press.
                    pane.reports_on_screen()
                };
                return self.load_output(plugin, refresh);
            }
            Message::PluginSuggestPicked(plugin, slot, value) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set_suggestion(slot, &value);
                    // Leaving the list up would leave it filtered by a
                    // name that is now the answer — a list of one.
                    pane.close_suggest();
                }
            }
            Message::PluginSuggestToggled(plugin, slot) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.toggle_suggest(slot);
                }
            }
            Message::PluginSectionSelected(plugin, index) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.select_section(index);
                }
                // Reaching a section is what makes its command-backed
                // controls visible; without this they show "Asking the
                // plug-in…" for ever. It is also why a chat client is
                // read only while its own section is open.
                return self.load_pending_outputs();
            }
            Message::PluginListToggled(plugin, control, member, on) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set_array_member(control, &member, on);
                }
            }
            Message::PluginListAll(plugin, control, on) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    pane.set_array_all(control, on);
                }
            }
            Message::PluginOutputRefresh(plugin, slot) => {
                let sharing = self
                    .plugins
                    .get(plugin)
                    .map(|p| p.sharing_command(slot))
                    .unwrap_or_default();
                return self.load_output(plugin, sharing);
            }
            Message::PluginOutputLoaded(plugin, slots, outcome) => {
                if let Some(pane) = self.plugins.get_mut(plugin) {
                    let state = match outcome {
                        Ok(text) => CommandOutput::Ready(text),
                        Err(why) => CommandOutput::Failed(why),
                    };
                    for slot in slots {
                        pane.set_output(slot, state.clone());
                    }
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
                // The checkbox renders the *effective* state: an empty
                // `[languages].active` means "every OS layout", so all
                // boxes start ticked. Unticking one there materialises
                // the list as everything-except-this, so the intent
                // survives a save; it is never auto-collapsed back to
                // empty.
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
                // Floor is the `MIN_UPDATE_INTERVAL_HOURS` the engine
                // clamps to at read time: the UI must not express a
                // value the app would silently ignore. Ceiling is a
                // week — beyond that "automatic updates" is a checkbox
                // that lies.
                let cur = i64::try_from(self.settings.updates.check_interval_hours).unwrap_or(24);
                let floor = i64::try_from(MIN_UPDATE_INTERVAL_HOURS).unwrap_or(1);
                let next = (cur + delta).clamp(floor, 24 * 7);
                self.settings.updates.check_interval_hours = u64::try_from(next).unwrap_or(24);
            }

            // ── Hotkeys ──────────────────────────────────────────
            Message::HotkeyRebindStart(kind) => {
                self.capturing = Some(kind);
                self.mod_capture = ModCapture::default();
            }
            Message::HotkeyRebindCancel => {
                self.capturing = None;
                self.mod_capture = ModCapture::default();
            }
            Message::HotkeyCaptured(combo) => return self.commit_hotkey(combo),
            Message::HotkeyModifier {
                role,
                pressed,
                held,
            } => {
                if self.capturing.is_none() {
                    return Task::none();
                }
                if let Some(combo) = mod_capture_step(&mut self.mod_capture, role, pressed, held) {
                    return self.commit_hotkey(combo);
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
                    // Snippet vs layout id vs URL: keeping the field
                    // would leave a half-typed value of the wrong kind.
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
            // All three selectors auto-flush the editor to disk first:
            // they overwrite the buffer with the freshly-loaded file, so
            // without this a click on another layout to "see what's
            // there" silently loses unsaved content.
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
                // Only semantic edits go dirty; cursor moves and scrolls
                // must not make us ask to save a buffer that was merely
                // looked at.
                if action.is_edit() {
                    self.wordlist_dirty = true;
                }
                self.wordlist_content.perform(action);
            }
            // ── Suggestions ──────────────────────────────────────
            Message::SuggestionsToggled(b) => self.settings.suggestions.enabled = b,
            // Guarded rather than trusted: the pane disables the
            // toggle where the session cannot do this, and a message
            // that arrives anyway must not write a setting the app
            // would then log a failure for on every press.
            Message::SelectionEnabledToggled(b) => {
                if self.selection_support.is_ok() {
                    self.settings.selection.enabled = b;
                }
            }
            Message::SuggestionMaxDelta(delta) => {
                // 1..=9 is the clamp `SuggestionSettings::max_clamped`
                // applies at read time — one digit key per entry — and
                // the UI must not express what the engine ignores.
                let cur = i64::try_from(self.settings.suggestions.max_suggestions).unwrap_or(5);
                let next = (cur + delta).clamp(1, 9);
                self.settings.suggestions.max_suggestions = usize::try_from(next).unwrap_or(5);
            }
            Message::SuggestionTimeoutDelta(delta) => {
                // The 3..=600 window `SuggestionSettings::timeout`
                // clamps to at read time.
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

            Message::TrayIconChoiceChanged(choice) => {
                self.settings.general.tray_icon = choice.config_value().to_owned();
            }

            Message::ManualOnlyChosen(manual) => {
                self.settings.general.paused = manual;
                self.conversion_chosen_here = true;
            }

            Message::ResetDefaults => self.settings = Settings::default(),
            Message::Reload => match SettingsStore::load_or_default() {
                Ok(fresh) => {
                    self.settings = fresh.snapshot();
                    // Reload means one thing everywhere: every
                    // on-disk-backed view resets to what is on disk.
                    // Unsaved editor content is discarded by design.
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
                // Footer Save saves EVERYTHING — `config.toml` and any
                // unsaved Wordlists edit; it carries more visual weight
                // than the per-pane Save, and a user who trusts it must
                // not lose the edit. Wordlist first, so the pane's own
                // banner reflects what happened before the global one.
                let wordlist_outcome = self.flush_wordlist_to_disk();
                if !matches!(wordlist_outcome, WordlistFlushOutcome::Nothing) {
                    self.wordlist_status = Some(banner_for_wordlist_save(wordlist_outcome));
                }
                // Straight off disk first: the tray may have written
                // the pause state since this window opened, and the
                // store's snapshot is this process's own copy.
                if let Err(e) = self.store.reload() {
                    warn!(?e, "could not re-read config.toml before saving");
                }
                let staged = with_runtime_state(
                    self.settings.clone(),
                    &self.store.snapshot(),
                    self.conversion_chosen_here,
                );
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
            Message::PluginOpenLink(url) => {
                // Re-checked at the point of opening, not only at
                // manifest load: a future path reaching this message
                // without the validator must still not be able to hand
                // `opener` a `file://` or a shell-ish scheme.
                if url.starts_with("https://") {
                    let _ = opener::open(&url);
                }
            }

            // ── Setup pane ─────────────────────────────────────────
            Message::SetupRecheck => {
                let before = self.setup.clone();
                self.setup = poltertype_input::setup::probe_setup(
                    &self.store.snapshot().updates.local_signing_identity,
                );
                // Say something either way: a button that silently
                // redraws the same screen reads as broken, and "still
                // not granted" is what the user most needs to hear.
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
                // `opener` hands http(s) docs links and macOS
                // `x-apple.systempreferences:` deep links alike to the
                // OS handler.
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
                // And open the pane regardless: when TCC has no record
                // the dialog appears over it, and when the app is not
                // in the list at all the user is already where the
                // "+" button lives — never a silent no-op
                // in front of an empty list.
                if let Some(url) = poltertype_input::setup::permission_settings_url(permission) {
                    let _ = opener::open(url);
                }
                self.setup = poltertype_input::setup::probe_setup(
                    &self.store.snapshot().updates.local_signing_identity,
                );
                self.setup_status = Some(SaveBanner {
                    text: "Asked the system. Approve it there, then press Check again.".to_owned(),
                    is_error: false,
                });
            }

            Message::SetupLocalSigning => {
                // Adopt-or-create in the keychain, then remember the name:
                // the updater reads it at swap time, and the re-probe below
                // is what flips the step to Done.
                let name = {
                    let configured = self.store.snapshot().updates.local_signing_identity;
                    if configured.is_empty() {
                        poltertype_input::setup::DEFAULT_LOCAL_SIGNING_IDENTITY.to_owned()
                    } else {
                        configured
                    }
                };
                match poltertype_input::setup::setup_local_signing(&name) {
                    Ok(()) => {
                        if let Err(e) =
                            self.store.update(|s| s.updates.local_signing_identity = name.clone())
                        {
                            warn!(?e, "could not remember the signing identity");
                        }
                        self.setup_status = Some(SaveBanner {
                            text: format!(
                                "Identity “{name}” is in your keychain — updates will keep \
                                 the permissions from now on."
                            ),
                            is_error: false,
                        });
                    }
                    Err(e) => {
                        warn!(%e, "local signing setup failed");
                        self.setup_status = Some(SaveBanner {
                            text: format!("Could not set up signing: {e}"),
                            is_error: true,
                        });
                    }
                }
                self.setup = poltertype_input::setup::probe_setup(
                    &self.store.snapshot().updates.local_signing_identity,
                );
            }

            Message::RestartApp => {
                // The settings window is a child of the tray process:
                // terminate the parent, start the bundle afresh, and
                // leave — keeping this window alive past the parent is
                // exactly the ghost that broke the updater relaunch.
                #[cfg(unix)]
                {
                    let parent = std::os::unix::process::parent_id();
                    if parent > 1 {
                        let _ = std::process::Command::new("kill")
                            .arg(parent.to_string())
                            .status();
                    }
                    if let Some(bundle) = std::env::current_exe().ok().and_then(|e| {
                        e.parent()?.parent()?.parent().map(std::path::Path::to_path_buf)
                    }) {
                        if bundle.extension().is_some_and(|x| x == "app") {
                            // Detached, and the open happens after WE
                            // are gone: while this settings process
                            // lives, LaunchServices reads the app as
                            // already running and open() merely
                            // activates this window — the exact ghost
                            // that used to eat the updater relaunch.
                            let _ = std::process::Command::new("sh")
                                .arg("-c")
                                .arg(format!(
                                    "sleep 2; open '{}'",
                                    bundle.display().to_string().replace('\'', "'\\''")
                                ))
                                .spawn();
                        }
                    }
                    std::process::exit(0);
                }
                #[cfg(not(unix))]
                {
                    warn!("restart from the pane is wired for unix only so far");
                }
            }

            Message::WindowCloseRequested(id) => {
                // Last chance to flush an unsaved wordlist edit.
                // Failures are logged but do not block the close: a
                // half-closed window is worse than one lost save.
                let _ = self.flush_wordlist_to_disk();
                return iced::window::close(id);
            }
        }
        Task::none()
    }

    /// Store a captured combination against the hotkey being rebound,
    /// or refuse it and say why.
    ///
    /// Refuse what the tray would refuse. A key is captured as the
    /// *character it produced*, so rebinding to a letter while a
    /// Cyrillic layout is active wrote `Ctrl+Shift+Ф` — which the
    /// reader rejects, and a rejected binding is silently replaced by
    /// the default. The rebind then looked accepted, the pane went on
    /// showing it, and the key did something else entirely.
    fn commit_hotkey(&mut self, combo: String) -> Task<Message> {
        let Some(kind) = self.capturing.take() else {
            return Task::none();
        };
        self.mod_capture = ModCapture::default();
        if !is_usable_hotkey(&combo) {
            warn!(?kind, %combo, "refusing a hotkey this build cannot read back");
            self.save_banner = Some(SaveBanner {
                text: format!(
                    "{combo} can't be used as a hotkey. \
                     Try a Latin letter or a function key — \
                     a key is stored by the character it types, \
                     so switch layout first if you meant a letter."
                ),
                is_error: true,
            });
            return Task::none();
        }
        info!(?kind, %combo, "captured new hotkey combo");
        match kind {
            HotkeyKind::Pause => self.settings.hotkeys.pause_toggle = combo,
            HotkeyKind::SwitchLast => self.settings.hotkeys.manual_switch_last = combo,
        }
        Task::none()
    }

    /// What the window has to do the moment it exists, before anybody
    /// clicks anything.
    pub(super) fn startup_task(&mut self) -> Task<Message> {
        if self.pane == Pane::Plugins {
            return self.load_pending_outputs();
        }
        Task::none()
    }

    /// Ask for every command-backed control on this pane that has not
    /// been asked yet.
    pub(super) fn load_pending_outputs(&mut self) -> Task<Message> {
        let wanted: Vec<usize> = (0..self.plugins.len())
            .filter(|i| {
                self.plugins
                    .get(*i)
                    .is_some_and(|p| !p.unasked_commands().is_empty())
            })
            .collect();
        Task::batch(
            wanted
                .into_iter()
                .flat_map(|plugin| {
                    let groups = self.plugins[plugin].unasked_by_command();
                    groups
                        .into_iter()
                        .map(|controls| self.load_output(plugin, controls))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
        )
    }

    /// Run one report command off the UI thread and deliver the answer
    /// as a message. A plain thread rather than anything cleverer: the
    /// work is one blocking wait on a child process, and the runtime
    /// under iced is not ours to assume.
    pub(super) fn load_output(&mut self, plugin: usize, slots: Vec<Slot>) -> Task<Message> {
        let Some(pane) = self.plugins.get_mut(plugin) else {
            return Task::none();
        };
        let Some(command) = slots
            .first()
            .and_then(|slot| pane.command_id(*slot))
            .map(str::to_owned)
        else {
            return Task::none();
        };
        let ext = pane.ext.clone();
        for slot in &slots {
            pane.set_output(*slot, CommandOutput::Loading);
        }

        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::plugins::read_report(&ext, &command));
        });
        Task::perform(
            async move {
                rx.await
                    .unwrap_or_else(|_| Err("the report task went away".to_owned()))
            },
            move |outcome| Message::PluginOutputLoaded(plugin, slots.clone(), outcome),
        )
    }

    /// The single shared "save the wordlist now" path — per-pane Save,
    /// footer Save and the auto-save before a profile / layout / kind
    /// switch all land here. Clears the dirty flag on success but never
    /// touches `wordlist_status`: the caller phrases the banner so it
    /// matches the trigger.
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
