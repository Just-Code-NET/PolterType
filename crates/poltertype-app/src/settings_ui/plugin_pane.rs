//! State behind the Plug-ins pane: one entry per installed extension,
//! holding the values its manifest declared and knowing how to write
//! them back.
//!
//! The pane edits *the plug-in's* config file, written and read by a
//! program we did not write, so two rules apply throughout: only the
//! keys the manifest declared are ever touched, and a write that cannot
//! be made cleanly is reported rather than forced. Everything else in
//! the file, comments included, comes back unchanged — that is
//! [`poltertype_core::plugins::write_setting`]'s whole job.

use std::path::{Path, PathBuf};

use poltertype_core::plugins::{
    ControlKind, DiscoveredExtension, PaneControl, SettingValue, read_setting, read_string_array,
    write_setting, write_string_array,
};
use tracing::warn;

/// What a control that has to *ask the plug-in* is showing right now.
///
/// Shared by the report, which shows the text, and the list, which
/// parses rows out of it: one cache, one place that knows a command has
/// been asked for. Three states and not two — a pane that shows an
/// empty box for both "waiting" and "got nothing" looks broken while it
/// is working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutput {
    Loading,
    /// It answered. May legitimately be empty text.
    Ready(String),
    /// It could not be asked, or it failed.
    Failed(String),
}

/// Which box on the pane is being talked about.
///
/// A control index alone is not enough: the fields inside a repeating
/// group's cards are controls too, they can carry a command of their
/// own, and each *card* holds its own half-typed text. So a box is
/// named by all three — control, declared field, card.
///
/// The command behind a field is asked once for the whole group rather
/// than once per card: which conversations exist is a question about the
/// chat client, not about the row. That answer is filed under
/// [`Self::asked`] — this slot with the card forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot {
    pub control: usize,
    /// Position in the control's `fields`. `None` — the control itself.
    pub field: Option<usize>,
    /// Which card of a repeating group. `None` — not in one.
    pub row: Option<usize>,
}

impl Slot {
    /// One of the plug-in's own controls.
    pub const fn control(control: usize) -> Self {
        Self {
            control,
            field: None,
            row: None,
        }
    }

    /// One field of one card.
    pub const fn field(control: usize, row: usize, field: usize) -> Self {
        Self {
            control,
            field: Some(field),
            row: Some(row),
        }
    }

    /// The same box with the card forgotten — what a command's answer is
    /// filed under, since one answer serves every card.
    pub const fn asked(self) -> Self {
        Self { row: None, ..self }
    }
}

/// The box the cursor is in.
///
/// Passed to [`PluginPane::flush_edits`] so that settling everything
/// else does not settle what somebody is halfway through typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typing {
    Control(usize),
    Record {
        control: usize,
        row: usize,
        field: String,
    },
}

/// One row of a list control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// What goes into the config array when the box is ticked.
    pub id: String,
    /// What the user reads.
    pub label: String,
    /// A line under it — where a row says what was measured about it.
    pub detail: String,
}

/// One plug-in as the pane sees it.
pub struct PluginPane {
    pub ext: DiscoveredExtension,
    /// The plug-in's own config file. May not exist yet — a plug-in is
    /// allowed to run entirely on its defaults.
    pub config_path: PathBuf,
    /// Current value per control, positionally. `None` means the file
    /// does not set it, so the plug-in's own default applies and we
    /// must not pretend to know what it is.
    pub values: Vec<Option<SettingValue>>,
    /// Result of the last edit, shown next to the plug-in.
    pub status: Option<String>,
    /// What each command-backed box is showing. A map rather than a
    /// vector of defaults because absent means "never asked", which is
    /// different from "asked, empty" — only one of them sends a command.
    ///
    /// Private, and written only through [`Self::set_output`], so the
    /// rows parsed out of it cannot be left describing an older answer.
    outputs: std::collections::HashMap<Slot, CommandOutput>,
    /// Which section is on screen, as a control index; `None` means the
    /// first one. One section at a time rather than an accordion:
    /// thirteen fold arrows over a page that is still metres long is not
    /// navigation.
    pub section: Option<usize>,
    /// What is in a text box right now, before it is a value.
    ///
    /// Without this the box can only show what the *file* holds, so a
    /// number cannot be cleared and a decimal cannot be typed — "0." is
    /// not a number, is therefore not written, and the box snaps back
    /// before the next character arrives.
    pub edits: std::collections::HashMap<usize, String>,
    /// Which members each list control's array currently holds, by
    /// control index — what decides whether a row's box is ticked.
    ///
    /// Cached because re-reading per *row* costs a `read_to_string` plus
    /// a whole format-preserving TOML parse each, measured at 78 µs
    /// against a 17 KB config: two room lists of 34 conversations read
    /// **1.2 MB and ran 68 TOML parses for every click**, since `view`
    /// rebuilds on every state change. Refreshed wherever the file can
    /// have changed — see [`Self::reload_arrays`].
    arrays: std::collections::HashMap<usize, Vec<String>>,
    /// The rows a command-backed box is drawing, parsed once when the
    /// plug-in's answer arrives rather than re-split on every rebuild.
    rows: std::collections::HashMap<Slot, Vec<ListRow>>,
    /// Which suggestion box has its list open, if any. One at a time,
    /// and inline: iced's own combo box draws an overlay sized to its
    /// options, which ninety-five conversations turned into a modal over
    /// the whole form.
    open_suggest: Option<Slot>,
    /// Which card's button is running. A row action takes seconds and
    /// changes the world, and a button that goes quiet for twenty
    /// seconds reads as one that did nothing.
    running_action: Option<(usize, usize)>,
    /// What each repeating-group control holds, by control index: one
    /// entry per row, each mapping the declared field names to what the
    /// file says. Cached for the same reason `arrays` is — reading a
    /// field at a time is a format-preserving TOML parse per field per
    /// row, on every rebuild.
    records: std::collections::HashMap<usize, Vec<RecordRow>>,
    /// What is being typed into a record's field, before it is a value —
    /// the per-row counterpart of `edits`: saving per keystroke would
    /// put every prefix of a message into a file the plug-in reads.
    record_edits: std::collections::HashMap<(usize, usize, String), String>,
}

/// One row of a repeating group: its declared fields, and what the file
/// holds for each. `None` for a field the row omits — the plug-in's own
/// default applies and this pane does not know it.
pub type RecordRow = std::collections::HashMap<String, Option<SettingValue>>;

impl PluginPane {
    /// Which controls need a command run and have not had one yet.
    ///
    /// Asked on the way in rather than on every draw: each costs a
    /// process and `view` rebuilds on every keystroke. Only the section
    /// on screen — reading a chat client's room list means talking to
    /// that application, and twelve unopened sections buy nothing.
    pub fn unasked_commands(&self) -> Vec<Slot> {
        self.command_slots()
            .into_iter()
            .filter(|slot| !self.outputs.contains_key(slot))
            .collect()
    }

    /// Every box on screen whose contents come from the plug-in:
    /// reports, tick-box lists and suggestion boxes, including the ones
    /// inside a repeating group's cards.
    fn command_slots(&self) -> Vec<Slot> {
        let mut slots = Vec::new();
        for (i, control) in self.ext.manifest.pane.iter().enumerate() {
            if !self.is_visible(i) {
                continue;
            }
            match control.kind {
                ControlKind::Report | ControlKind::List => slots.push(Slot::control(i)),
                ControlKind::Suggest if !control.command.trim().is_empty() => {
                    slots.push(Slot::control(i));
                }
                ControlKind::Records => {
                    for (f, field) in control.fields.iter().enumerate() {
                        if field.kind == ControlKind::Suggest && !field.command.trim().is_empty() {
                            slots.push(Slot {
                                control: i,
                                field: Some(f),
                                row: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        slots
    }

    /// The command behind one box, if it has one.
    pub fn command_id(&self, slot: Slot) -> Option<&str> {
        let control = self.control(slot.control)?;
        let declared = match slot.field {
            Some(f) => control.fields.get(f)?,
            None => control,
        };
        let command = declared.command.trim();
        (!command.is_empty()).then_some(command)
    }

    /// Every section heading, in declaration order.
    pub fn sections(&self) -> Vec<usize> {
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ControlKind::Section)
            .map(|(i, _)| i)
            .collect()
    }

    /// The section on screen: what was chosen, or the first one.
    pub fn selected_section(&self) -> Option<usize> {
        match self.section {
            Some(i) if matches!(self.control(i).map(|c| c.kind), Some(ControlKind::Section)) => {
                Some(i)
            }
            _ => self.sections().first().copied(),
        }
    }

    /// Is this control on screen?
    ///
    /// A control belongs to the nearest [`ControlKind::Section`] above
    /// it. Controls declared *before* the first section belong to none
    /// and are always shown, which is also what makes a plug-in with no
    /// sections render everything.
    pub fn is_visible(&self, index: usize) -> bool {
        let controls = &self.ext.manifest.pane;
        let Some(selected) = self.selected_section() else {
            return true;
        };
        if index == selected {
            return true;
        }
        if matches!(
            controls.get(index).map(|c| c.kind),
            Some(ControlKind::Section)
        ) {
            return false;
        }
        controls[..index.min(controls.len())]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| c.kind == ControlKind::Section)
            .is_none_or(|(i, _)| i == selected)
    }

    /// [`Self::unasked_commands`], grouped so each command runs once.
    pub fn unasked_by_command(&self) -> Vec<Vec<Slot>> {
        let mut groups: Vec<(String, Vec<Slot>)> = Vec::new();
        for slot in self.unasked_commands() {
            let Some(command) = self.command_id(slot).map(str::to_owned) else {
                continue;
            };
            match groups.iter_mut().find(|(id, _)| *id == command) {
                Some((_, members)) => members.push(slot),
                None => groups.push((command, vec![slot])),
            }
        }
        groups.into_iter().map(|(_, members)| members).collect()
    }

    /// Every box fed by the same command as this one, itself included —
    /// what a Refresh should update, since they are all showing one
    /// answer.
    pub fn sharing_command(&self, slot: Slot) -> Vec<Slot> {
        let Some(command) = self.command_id(slot).map(str::to_owned) else {
            return Vec::new();
        };
        self.command_slots()
            .into_iter()
            .filter(|other| self.command_id(*other) == Some(command.as_str()))
            .collect()
    }

    /// Show one section.
    ///
    /// Also the moment to re-read the arrays: reaching a section is the
    /// user's own step, and it is where a change made in an editor
    /// since the window opened gets picked up.
    pub fn select_section(&mut self, index: usize) {
        self.section = Some(index);
        self.reload_arrays();
    }

    /// What a command-backed box is showing now.
    ///
    /// The one way to set an output, so the parsed rows cannot fall out
    /// of step with the text they came from.
    pub fn set_output(&mut self, slot: Slot, state: CommandOutput) {
        let slot = slot.asked();
        self.rows.remove(&slot);
        if let CommandOutput::Ready(text) = &state {
            self.rows.insert(slot, parse_list_rows(text));
        }
        self.outputs.insert(slot, state);
    }

    /// What a command-backed box is showing, for the pane to draw.
    pub fn output(&self, slot: Slot) -> Option<&CommandOutput> {
        self.outputs.get(&slot.asked())
    }

    /// The rows behind one box: `id`, its label, and a line of detail.
    pub fn list_rows(&self, slot: Slot) -> &[ListRow] {
        self.rows.get(&slot.asked()).map_or(&[], Vec::as_slice)
    }

    /// What a suggestion box offers: what the manifest named, then what
    /// the plug-in answered, in that order and without repeats.
    ///
    /// The plug-in's rows contribute their **id**, never their label:
    /// what is picked is what is written, and a friendlier name would
    /// make the box store something other than what it shows. The
    /// *detail* comes along beside it, since a name alone often cannot
    /// answer "which of these ninety-five".
    pub fn suggestions(&self, slot: Slot) -> Vec<(String, String)> {
        let Some(control) = self.control(slot.control) else {
            return Vec::new();
        };
        let declared = match slot.field {
            Some(f) => match control.fields.get(f) {
                Some(field) => field,
                None => return Vec::new(),
            },
            None => control,
        };
        let mut out: Vec<(String, String)> = declared
            .options
            .iter()
            .map(|o| (o.value().to_owned(), o.detail().to_owned()))
            .collect();
        for row in self.list_rows(slot) {
            if !out.iter().any(|(seen, _)| *seen == row.id) {
                out.push((row.id.clone(), row.detail.clone()));
            }
        }
        out
    }

    /// The ones worth drawing under the box right now: everything when
    /// nothing has been typed, what matches when something has.
    ///
    /// Matched case-insensitively on the value — the same loose match
    /// the plug-ins' own room allow-lists use, so picking from this list
    /// and typing the name by hand mean the same thing.
    pub fn suggestions_matching(&self, slot: Slot) -> Vec<(String, String)> {
        let needle = self.pending(slot).unwrap_or_default().trim().to_lowercase();
        self.suggestions(slot)
            .into_iter()
            .filter(|(value, _)| needle.is_empty() || value.to_lowercase().contains(&needle))
            .collect()
    }

    /// What is being typed into a box, if anything — the difference
    /// between "opened to look" and "narrowing".
    pub fn pending(&self, slot: Slot) -> Option<String> {
        match (slot.row, slot.field) {
            (Some(row), Some(field)) => {
                let key = self
                    .control(slot.control)?
                    .fields
                    .get(field)
                    .map(|f| f.key.clone())?;
                self.record_edits.get(&(slot.control, row, key)).cloned()
            }
            _ => self.edits.get(&slot.control).cloned(),
        }
    }

    /// Is this box's list open? Typing opens it — narrowing a list you
    /// cannot see is not narrowing anything.
    pub fn suggest_open(&self, slot: Slot) -> bool {
        self.open_suggest == Some(slot) || self.pending(slot).is_some()
    }

    /// The button beside the box, which opens the list without typing,
    /// and closes it again.
    pub fn toggle_suggest(&mut self, slot: Slot) {
        self.open_suggest = if self.open_suggest == Some(slot) {
            None
        } else {
            Some(slot)
        };
    }

    pub fn close_suggest(&mut self) {
        self.open_suggest = None;
    }

    /// Re-read every list control's array from the plug-in's config.
    ///
    /// One read and one parse per list control, on a step the user took
    /// — not per row and not per frame. Another program owns this file,
    /// so the answer still comes from disk rather than from what this
    /// pane last wrote.
    fn reload_arrays(&mut self) {
        let keys: Vec<(usize, String)> = self
            .ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ControlKind::List && !c.key.is_empty())
            .map(|(i, c)| (i, c.key.clone()))
            .collect();
        if keys.is_empty() {
            return;
        }
        let text = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        self.arrays = keys
            .into_iter()
            .map(|(i, key)| (i, read_string_array(&text, &key)))
            .collect();
    }

    /// Read the current values for one extension.
    ///
    /// `config_root` is the directory holding *per-application* config
    /// directories — the parent of ours. A plug-in is a separate
    /// program, so its config sits beside PolterType's, not inside it.
    pub fn load(ext: DiscoveredExtension, config_root: &Path) -> Self {
        let config_path = config_root
            .join(&ext.id)
            .join(if ext.manifest.config_file.is_empty() {
                "config.toml"
            } else {
                &ext.manifest.config_file
            });
        let text = std::fs::read_to_string(&config_path).unwrap_or_default();
        let values = ext
            .manifest
            .pane
            .iter()
            .map(|c| {
                if c.key.is_empty() {
                    None
                } else if c.kind == ControlKind::Strings {
                    // An array has no `SettingValue`, and it does not
                    // need one: the box shows the members joined, and
                    // what is written back is always a fresh array.
                    let members = read_string_array(&text, &c.key);
                    (!members.is_empty()).then(|| SettingValue::Text(members.join(", ")))
                } else {
                    read_setting(&text, &c.key)
                }
            })
            .collect();
        let mut pane = Self {
            ext,
            config_path,
            values,
            status: None,
            outputs: std::collections::HashMap::new(),
            section: None,
            edits: std::collections::HashMap::new(),
            arrays: std::collections::HashMap::new(),
            rows: std::collections::HashMap::new(),
            open_suggest: None,
            records: std::collections::HashMap::new(),
            record_edits: std::collections::HashMap::new(),
            running_action: None,
        };
        pane.reload_arrays();
        pane.reload_records();
        pane
    }

    /// Re-read every repeating group from the file.
    ///
    /// Called wherever the file can have changed under us, like
    /// [`Self::reload_arrays`]: add, remove and set-field all rewrite
    /// the document, and a stale cache would draw the deleted row.
    fn reload_records(&mut self) {
        let groups: Vec<(usize, String, Vec<String>)> = self
            .ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ControlKind::Records && !c.key.trim().is_empty())
            .map(|(i, c)| {
                (
                    i,
                    c.key.clone(),
                    c.fields.iter().map(|f| f.key.clone()).collect(),
                )
            })
            .collect();
        let text = if groups.is_empty() {
            String::new()
        } else {
            std::fs::read_to_string(&self.config_path).unwrap_or_default()
        };
        self.records = groups
            .into_iter()
            .map(|(i, key, fields)| {
                let n = poltertype_core::plugins::count_records(&text, &key);
                let rows = (0..n)
                    .map(|row| {
                        fields
                            .iter()
                            .map(|f| {
                                (
                                    f.clone(),
                                    poltertype_core::plugins::read_record_field(
                                        &text, &key, row, f,
                                    ),
                                )
                            })
                            .collect()
                    })
                    .collect();
                (i, rows)
            })
            .collect();
    }

    /// The rows a repeating group is drawing.
    pub fn record_rows(&self, index: usize) -> &[RecordRow] {
        self.records.get(&index).map_or(&[], Vec::as_slice)
    }

    /// What one field of one row should show: what is being typed, else
    /// what the file holds, else nothing.
    pub fn record_display(&self, index: usize, row: usize, field: &str) -> Option<String> {
        if let Some(raw) = self.record_edits.get(&(index, row, field.to_owned())) {
            return Some(raw.clone());
        }
        self.records
            .get(&index)?
            .get(row)?
            .get(field)?
            .as_ref()
            .map(SettingValue::as_display)
    }

    /// The stored value of one field, for a control that renders a value
    /// rather than text — a toggle, a chosen option.
    pub fn record_value(&self, index: usize, row: usize, field: &str) -> Option<SettingValue> {
        self.records.get(&index)?.get(row)?.get(field)?.clone()
    }

    /// The report controls on screen, one slot each.
    ///
    /// What to re-ask after a row action: a report describes state the
    /// action just changed. A conversation list does not — re-asking one
    /// reads a chat client's sidebar for an unrelated button press.
    pub fn reports_on_screen(&self) -> Vec<Slot> {
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(i, c)| c.kind == ControlKind::Report && self.is_visible(*i))
            .map(|(i, _)| Slot::control(i))
            .collect()
    }

    /// Is this card's button running right now?
    pub fn action_running(&self, index: usize, row: usize) -> bool {
        self.running_action == Some((index, row))
    }

    /// Anything running at all — one at a time, because these steal
    /// focus and two of them would type into each other's window.
    pub fn any_action_running(&self) -> bool {
        self.running_action.is_some()
    }

    pub fn set_action_running(&mut self, running: Option<(usize, usize)>) {
        self.running_action = running;
    }

    /// What one card calls itself: the value of the field the manifest
    /// named as the group's `id_field`.
    ///
    /// `None` while that field is empty: a row action runs against a
    /// name the plug-in knows, and a blank one names nothing.
    pub fn record_id(&self, index: usize, row: usize) -> Option<String> {
        let control = self.control(index)?;
        let field = control.id_field.trim();
        if field.is_empty() {
            return None;
        }
        let id = self.record_display(index, row, field)?;
        (!id.trim().is_empty()).then(|| id.trim().to_owned())
    }

    /// One of a suggestion box's candidates was picked — write it,
    /// wherever that box lives.
    pub fn set_suggestion(&mut self, slot: Slot, value: &str) {
        let picked = SettingValue::Text(value.to_owned());
        match (slot.row, slot.field) {
            (Some(row), Some(field)) => {
                let Some(key) = self
                    .control(slot.control)
                    .and_then(|c| c.fields.get(field))
                    .map(|f| f.key.clone())
                else {
                    return;
                };
                self.set_record(slot.control, row, &key, picked);
            }
            _ => self.set(slot.control, picked),
        }
    }

    /// Note what is being typed into a record's field. Written to the
    /// file by [`Self::flush_edits`], not here.
    pub fn set_record_text(&mut self, index: usize, row: usize, field: &str, raw: String) {
        self.record_edits
            .insert((index, row, field.to_owned()), raw);
    }

    /// Write one field of one row.
    pub fn set_record(&mut self, index: usize, row: usize, field: &str, value: SettingValue) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        let key = control.key.clone();
        if key.is_empty() {
            return;
        }
        // Picking from a list settles that box. Anything left half-typed
        // in it is what the picking replaced, and flushing it afterwards
        // would put it back over the choice.
        self.record_edits.remove(&(index, row, field.to_owned()));
        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match poltertype_core::plugins::write_record_field(&current, &key, row, field, &value) {
            Ok(updated) => {
                if self.write(updated) {
                    self.reload_records();
                }
            }
            Err(e) => self.status = Some(format!("{e}")),
        }
    }

    /// Append an empty row.
    pub fn add_record(&mut self, index: usize) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        let key = control.key.clone();
        if key.is_empty() {
            return;
        }
        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match poltertype_core::plugins::add_record(&current, &key) {
            Ok(updated) => {
                if self.write(updated) {
                    self.reload_records();
                }
            }
            Err(e) => self.status = Some(format!("{e}")),
        }
    }

    /// Delete a row, and everything being typed into it.
    pub fn remove_record(&mut self, index: usize, row: usize) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        let key = control.key.clone();
        if key.is_empty() {
            return;
        }
        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match poltertype_core::plugins::remove_record(&current, &key, row) {
            Ok(updated) => {
                if self.write(updated) {
                    // Half-typed text belonging to rows that just
                    // shifted up would otherwise settle into the wrong
                    // row.
                    self.record_edits.retain(|(i, _, _), _| *i != index);
                    // The list that was open belonged to a card that has
                    // just shifted; it would reopen under the wrong one.
                    self.open_suggest = None;
                    self.reload_records();
                }
            }
            Err(e) => self.status = Some(format!("{e}")),
        }
    }

    /// What a text-shaped control's box should show: what is being
    /// typed, else what the file holds, else nothing (and the box shows
    /// its "plug-in default" placeholder).
    pub fn display_of(&self, index: usize) -> Option<String> {
        if let Some(raw) = self.edits.get(&index) {
            return Some(raw.clone());
        }
        self.values
            .get(index)
            .and_then(|v| v.as_ref())
            .map(SettingValue::as_display)
    }

    /// A text-shaped control was typed into. Held, not written.
    pub fn set_text(&mut self, index: usize, raw: String) {
        self.edits.insert(index, raw);
    }

    /// Write everything typed since the last flush, except the box the
    /// user is still in.
    ///
    /// Deferring the write is the point. Saving on every keystroke puts
    /// every prefix of what is typed into a file the plug-in is
    /// reading: a threshold on its way from `0.9` to `0.95` passes
    /// through `0`, and for the length of a keystroke the gate is wide
    /// open. So a value settles when the user does something else, and
    /// at the latest when the window closes.
    ///
    /// Text that is not yet a value of the right shape stays in the box
    /// and out of the file — writing `1` for a half-typed `1.5` would
    /// be worse than waiting.
    pub fn flush_edits(&mut self, still_typing: Option<&Typing>) {
        let held = match still_typing {
            Some(Typing::Control(index)) => Some(*index),
            _ => None,
        };
        let pending: Vec<(usize, String)> = self
            .edits
            .iter()
            .filter(|(i, _)| Some(**i) != held)
            .map(|(i, raw)| (*i, raw.clone()))
            .collect();

        for (index, raw) in pending {
            let Some(kind) = self.control(index).map(|c| c.kind) else {
                continue;
            };
            let trimmed = raw.trim().to_owned();
            let settled = match kind {
                ControlKind::Number => match trimmed.parse::<i64>() {
                    Ok(n) => {
                        self.set(index, SettingValue::Int(n));
                        true
                    }
                    Err(_) => false,
                },
                ControlKind::Decimal => match trimmed.parse::<f64>() {
                    Ok(f) if f.is_finite() => {
                        self.set(index, SettingValue::Float(f));
                        true
                    }
                    _ => false,
                },
                ControlKind::Strings => {
                    self.set_strings(index, &trimmed);
                    true
                }
                _ => {
                    self.set(index, SettingValue::Text(trimmed));
                    true
                }
            };
            if settled {
                self.edits.remove(&index);
            }
        }
        self.flush_record_edits(still_typing);
    }

    /// The same deferral for the boxes inside a repeating group. A
    /// card's box is addressed by row and field where a control is
    /// addressed by an index, so [`Typing`] has to name either kind —
    /// a caller that could only name a control index would settle the
    /// previous keystroke on every keystroke.
    fn flush_record_edits(&mut self, still_typing: Option<&Typing>) {
        let held = match still_typing {
            Some(Typing::Record {
                control,
                row,
                field,
            }) => Some((*control, *row, field.clone())),
            _ => None,
        };
        let pending: Vec<((usize, usize, String), String)> = self
            .record_edits
            .iter()
            .filter(|(k, _)| held.as_ref() != Some(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for ((index, row, field), raw) in pending {
            let kind = self
                .ext
                .manifest
                .pane
                .get(index)
                .and_then(|c| c.fields.iter().find(|f| f.key == field))
                .map(|f| f.kind);
            let trimmed = raw.trim().to_owned();
            let settled = match kind {
                Some(ControlKind::Number) => match trimmed.parse::<i64>() {
                    Ok(n) => {
                        self.set_record(index, row, &field, SettingValue::Int(n));
                        true
                    }
                    Err(_) => false,
                },
                Some(ControlKind::Decimal) => match trimmed.parse::<f64>() {
                    Ok(f) if f.is_finite() => {
                        self.set_record(index, row, &field, SettingValue::Float(f));
                        true
                    }
                    _ => false,
                },
                // A field the manifest does not declare cannot be
                // written anywhere sensible; drop what was typed rather
                // than keep retrying it for the life of the window.
                None => true,
                _ => {
                    self.set_record(index, row, &field, SettingValue::Text(trimmed));
                    true
                }
            };
            if settled {
                self.record_edits.remove(&(index, row, field));
            }
        }
    }

    /// Write the comma-separated box back as an array.
    ///
    /// Empty members are dropped, so a trailing comma while typing does
    /// not put `""` in the list — which, for the substring matching
    /// these lists usually feed, would match everything.
    fn set_strings(&mut self, index: usize, raw: &str) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        if control.key.is_empty() {
            return;
        }
        let key = control.key.clone();
        let members: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match write_string_array(&current, &key, &members) {
            Ok(updated) => {
                if self.write(updated) {
                    self.values[index] = Some(SettingValue::Text(members.join(", ")));
                }
            }
            Err(e) => {
                warn!(key = %key, "cannot edit plug-in config list: {e}");
                self.status = Some(format!("Could not change {key}: {e}"));
            }
        }
    }

    /// The value to render for a control: what the file says, or the
    /// neutral default for its kind.
    pub fn value_of(&self, index: usize) -> SettingValue {
        match self.values.get(index).and_then(|v| v.clone()) {
            Some(v) => v,
            None => match self.ext.manifest.pane.get(index).map(|c| c.kind) {
                Some(ControlKind::Toggle) => SettingValue::Bool(false),
                Some(ControlKind::Number) => SettingValue::Int(0),
                Some(ControlKind::Decimal) => SettingValue::Float(0.0),
                _ => SettingValue::Text(String::new()),
            },
        }
    }

    pub fn control(&self, index: usize) -> Option<&PaneControl> {
        self.ext.manifest.pane.get(index)
    }

    /// Is `member` currently in the array this list control edits?
    ///
    /// Answered from [`Self::arrays`], which every write here refreshes
    /// — this runs once per row on every view rebuild and must not
    /// touch the disk.
    pub fn in_array(&self, index: usize, member: &str) -> bool {
        self.arrays
            .get(&index)
            .is_some_and(|members| members.iter().any(|entry| entry == member))
    }

    /// Add `member` to this control's array, or take it out.
    pub fn set_array_member(&mut self, index: usize, member: &str, present: bool) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        if control.key.is_empty() {
            return;
        }
        let key = control.key.clone();
        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match poltertype_core::plugins::set_array_member(&current, &key, member, present) {
            Ok(updated) => {
                self.write(updated);
            }
            Err(e) => {
                warn!(key = %key, "cannot edit plug-in config array: {e}");
                self.status = Some(format!("Could not change {key}: {e}"));
            }
        }
    }

    /// Tick, or untick, every row this control is currently offering.
    ///
    /// The rows on screen and nothing else. A list can hold names the
    /// plug-in did not offer this time — a conversation in a client that
    /// is not running, one typed by hand — and the user is acting on the
    /// list they can see, so what is invisible is left alone.
    ///
    /// One write for the whole set, so the file another program is
    /// reading is never caught half-updated.
    pub fn set_array_all(&mut self, index: usize, present: bool) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        if control.key.is_empty() {
            return;
        }
        let key = control.key.clone();
        let members: Vec<String> = self
            .list_rows(Slot::control(index))
            .iter()
            .map(|row| row.id.clone())
            .collect();
        if members.is_empty() {
            return;
        }
        let borrowed: Vec<&str> = members.iter().map(String::as_str).collect();
        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match poltertype_core::plugins::set_array_members(&current, &key, &borrowed, present) {
            Ok(updated) => {
                self.write(updated);
            }
            Err(e) => {
                warn!(key = %key, "cannot edit plug-in config array: {e}");
                self.status = Some(format!("Could not change {key}: {e}"));
            }
        }
    }

    /// Write the plug-in's config file back, reporting either way, and
    /// say whether it landed.
    ///
    /// The one place the file is written, so also the one place the
    /// cached arrays are brought back in step — a ticked box that
    /// re-read nothing springs back open on the next frame.
    fn write(&mut self, updated: String) -> bool {
        if let Some(dir) = self.config_path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                self.status = Some(format!("Could not create {}: {e}", dir.display()));
                return false;
            }
        }
        match std::fs::write(&self.config_path, updated) {
            Ok(()) => {
                self.status = Some(format!("Saved to {}", self.config_path.display()));
                self.reload_arrays();
                true
            }
            Err(e) => {
                warn!(path = %self.config_path.display(), "cannot write plug-in config: {e}");
                self.status = Some(format!(
                    "Could not write {}: {e}",
                    self.config_path.display()
                ));
                false
            }
        }
    }

    /// Write one control's value into the plug-in's config file.
    ///
    /// Reads, edits and writes on the spot rather than batching: the
    /// plug-in may be running and watching that file.
    pub fn set(&mut self, index: usize, value: SettingValue) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        if control.key.is_empty() {
            return;
        }
        let key = control.key.clone();
        // Picking from a list settles the box; see [`Self::set_record`].
        self.edits.remove(&index);

        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match write_setting(&current, &key, &value) {
            Ok(updated) => {
                if self.write(updated) {
                    self.values[index] = Some(value);
                }
            }
            Err(e) => {
                // The plug-in's file is not something we may rewrite on
                // a guess — say what is wrong and change nothing.
                self.status = Some(format!("{e}"));
            }
        }
    }
}

/// Parse a list command's output into rows.
///
/// Tab-separated and tolerant in the same way the state protocol is —
/// a line with no tab is an id that is its own label, extra fields are
/// ignored, blank lines skipped. A plug-in should be able to print
/// something readable without it becoming a parsing contract.
fn parse_list_rows(text: &str) -> Vec<ListRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next().unwrap_or_default().trim().to_owned();
            let label = fields.next().unwrap_or_default().trim();
            let detail = fields.next().unwrap_or_default().trim();
            ListRow {
                label: if label.is_empty() {
                    id.clone()
                } else {
                    label.to_owned()
                },
                id,
                detail: detail.to_owned(),
            }
        })
        .filter(|row| !row.id.is_empty())
        .collect()
}

/// Load every discovered extension that actually declares a pane.
///
/// A plug-in with no controls gets no section: an empty box with a
/// name in it tells the user nothing and makes the list longer.
pub fn load_all(extensions: Vec<DiscoveredExtension>, config_root: &Path) -> Vec<PluginPane> {
    extensions
        .into_iter()
        .filter(|e| !e.manifest.pane.is_empty())
        .map(|e| PluginPane::load(e, config_root))
        .collect()
}

#[cfg(test)]
#[path = "plugin_pane_tests.rs"]
mod tests;
