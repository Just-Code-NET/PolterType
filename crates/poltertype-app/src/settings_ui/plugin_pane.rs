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
/// parses rows out of it, so there is one cache and one place that
/// knows a command has been asked for.
///
/// Three states and not two: "asked, waiting" reads very differently
/// from "asked, got nothing", and a pane that shows an empty box for
/// both looks broken while it is working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutput {
    /// The command is running.
    Loading,
    /// It answered. May legitimately be empty text.
    Ready(String),
    /// It could not be asked, or it failed.
    Failed(String),
}

/// Which box on the pane is being talked about.
///
/// A control index was enough while everything that could be asked for,
/// typed into or refreshed was one of the plug-in's own controls. A
/// repeating group broke that: the fields inside its cards are controls
/// too, they can have a command of their own, and each *card* has its
/// own box holding its own half-typed text. So a box is named by all
/// three — which control, which of its declared fields, and which card.
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
/// else does not settle what somebody is halfway through typing. See
/// that method for why writing a half-typed value is worse than waiting.
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
    /// What each command-backed box is showing. Absent means it has not
    /// been asked yet — which is why it is a map and not a vector of
    /// defaults: "never asked" and "asked, empty" are different, and
    /// only one of them should send a command.
    ///
    /// Private, and written only through [`Self::set_output`], so the
    /// rows parsed out of it cannot be left describing an older answer.
    outputs: std::collections::HashMap<Slot, CommandOutput>,
    /// Which section is on screen, as a control index. `None` before
    /// anything is chosen, which means "the first one".
    ///
    /// One section at a time rather than an accordion: a plug-in with
    /// a hundred settings has thirteen sections, and thirteen fold
    /// arrows over a page that is still metres long is not navigation.
    pub section: Option<usize>,
    /// What is in a text box right now, before it is a value.
    ///
    /// Without this the box can only ever show what the *file* holds,
    /// so a number cannot be cleared and a decimal cannot be typed —
    /// "0." is not a number, is therefore not written, and the box
    /// snaps back before the next character arrives.
    pub edits: std::collections::HashMap<usize, String>,
    /// Which members each list control's array currently holds, by
    /// control index — what decides whether a row's box is ticked.
    ///
    /// Cached because the answer used to be re-read from the file per
    /// *row*: one `read_to_string` plus a whole format-preserving TOML
    /// parse each, measured at 78 µs against a 17 KB config. `view`
    /// rebuilds on every state change, so a chat plug-in showing two
    /// room lists of 34 conversations read **1.2 MB and ran 68 TOML
    /// parses for every click** — measured on this pane, against a file
    /// the click itself had just written. Refreshed wherever the file
    /// can have changed — see [`Self::reload_arrays`].
    arrays: std::collections::HashMap<usize, Vec<String>>,
    /// The rows a command-backed box is drawing, parsed once when the
    /// plug-in's answer arrives rather than re-split on every rebuild.
    rows: std::collections::HashMap<Slot, Vec<ListRow>>,
    /// The searching state behind every suggestion box: what has been
    /// typed into it, and which candidates that leaves.
    ///
    /// It has to survive a rebuild — `view` runs again on every
    /// keystroke, and a state built fresh each time would drop the
    /// filter the keystroke was narrowing. So it is rebuilt only when
    /// the *candidates* change, which `combo_sources` is what detects.
    combos: std::collections::HashMap<Slot, iced::widget::combo_box::State<String>>,
    /// What each of those states was built from, so an answer that says
    /// the same thing twice does not clear a half-typed name.
    combo_sources: std::collections::HashMap<Slot, Vec<String>>,
    /// What each repeating-group control holds, by control index: one
    /// entry per row, each mapping the declared field names to what the
    /// file says. Cached for the same reason `arrays` is — `view`
    /// rebuilds on every keystroke, and reading a field at a time would
    /// be a whole format-preserving TOML parse per field per row.
    records: std::collections::HashMap<usize, Vec<RecordRow>>,
    /// What is being typed into a record's field, before it is a value —
    /// the per-row counterpart of `edits`, and there for the same reason:
    /// a pane that saved on every keystroke would put every prefix of a
    /// message into a file the plug-in is reading.
    record_edits: std::collections::HashMap<(usize, usize, String), String>,
}

/// One row of a repeating group: its declared fields, and what the file
/// holds for each. `None` for a field the row omits — the plug-in's own
/// default applies and this pane does not know it.
pub type RecordRow = std::collections::HashMap<String, Option<SettingValue>>;

impl PluginPane {
    /// Which controls need a command run and have not had one yet.
    ///
    /// The pane asks on the way in rather than on every draw: each of
    /// these costs a process, and `view` is rebuilt on every state
    /// change — every click, every keystroke in a box. Only the
    /// section on screen is asked — reading a chat client's room list
    /// means talking to that application, and doing it for twelve
    /// sections nobody opened is a cost with nothing to show for it.
    pub fn unasked_commands(&self) -> Vec<Slot> {
        self.command_slots()
            .into_iter()
            .filter(|slot| !self.outputs.contains_key(slot))
            .collect()
    }

    /// Every box on screen whose contents come from the plug-in: the
    /// reports and tick-box lists, and the suggestion boxes — including
    /// the ones inside a repeating group's cards, which is where a
    /// conversation gets picked.
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
    /// and are always shown — which is also what makes a plug-in with
    /// no sections at all render exactly as it used to.
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
        self.sync_combos();
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
    /// The plug-in's rows contribute their **id**, never their label —
    /// what is picked is what is written, and a friendlier name in the
    /// list would be a box that stores something other than what it
    /// shows.
    fn suggestions(&self, slot: Slot) -> Vec<String> {
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
        let mut out: Vec<String> = declared
            .options
            .iter()
            .map(|o| o.value().to_owned())
            .collect();
        for row in self.list_rows(slot) {
            if !out.contains(&row.id) {
                out.push(row.id.clone());
            }
        }
        out
    }

    /// The searching state behind one suggestion box.
    pub fn combo(&self, slot: Slot) -> Option<&iced::widget::combo_box::State<String>> {
        self.combos.get(&slot)
    }

    /// Build a searching state for every suggestion box that needs one,
    /// and leave the rest alone.
    ///
    /// "Leave the rest alone" is the whole subtlety. Rebuilding a state
    /// resets what has been typed into it, and this runs whenever an
    /// answer arrives or a card is added — so a state is replaced only
    /// when the candidates behind it actually changed.
    fn sync_combos(&mut self) {
        let mut wanted: Vec<Slot> = Vec::new();
        for (i, control) in self.ext.manifest.pane.iter().enumerate() {
            match control.kind {
                ControlKind::Suggest => wanted.push(Slot::control(i)),
                ControlKind::Records => {
                    let rows = self.records.get(&i).map_or(0, Vec::len);
                    for (f, field) in control.fields.iter().enumerate() {
                        if field.kind != ControlKind::Suggest {
                            continue;
                        }
                        for row in 0..rows {
                            wanted.push(Slot::field(i, row, f));
                        }
                    }
                }
                _ => {}
            }
        }

        self.combos.retain(|slot, _| wanted.contains(slot));
        self.combo_sources.retain(|slot, _| wanted.contains(slot));
        for slot in wanted {
            let options = self.suggestions(slot);
            if self.combo_sources.get(&slot) == Some(&options) {
                continue;
            }
            self.combos
                .insert(slot, iced::widget::combo_box::State::new(options.clone()));
            self.combo_sources.insert(slot, options);
        }
    }

    /// Re-read every list control's array from the plug-in's config.
    ///
    /// One read and one parse per list control, on a step the user
    /// took — not per row and not per frame. Another program owns this
    /// file, so the answer is still taken from disk rather than
    /// inferred from what this pane last wrote.
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
    /// directories — the parent of ours — because a plug-in keeps its
    /// config beside PolterType's rather than inside it. It is a
    /// separate program; its settings are not a subsection of ours.
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
            combos: std::collections::HashMap::new(),
            combo_sources: std::collections::HashMap::new(),
            records: std::collections::HashMap::new(),
            record_edits: std::collections::HashMap::new(),
        };
        pane.reload_arrays();
        pane.reload_records();
        pane
    }

    /// Re-read every repeating group from the file.
    ///
    /// Called wherever the file can have changed under us, exactly like
    /// [`Self::reload_arrays`]: adding a row, removing one, and setting a
    /// field all rewrite the document, and a stale cache would draw the
    /// row that was just deleted.
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
        // A card that has just appeared needs a searching state; one
        // that has just gone must not leave a stale one behind for the
        // card that shifted up into its place.
        self.sync_combos();
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

    /// What one card calls itself: the value of the field the manifest
    /// named as the group's `id_field`.
    ///
    /// `None` while that field is empty. A row action is a command run
    /// against a name the plug-in knows, and a blank one would be a
    /// command run against nothing at all.
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
                    // Half-typed text belonging to rows that have just
                    // shifted up would otherwise be flushed into the
                    // wrong row the next time anything settles — and a
                    // searching box would go on showing what was being
                    // looked for in the card above it.
                    self.record_edits.retain(|(i, _, _), _| *i != index);
                    self.combos.retain(|slot, _| slot.control != index);
                    self.combo_sources.retain(|slot, _| slot.control != index);
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
    /// Deferring the write is the point. A pane that saved on every
    /// keystroke — which this one used to do — puts every prefix of
    /// what is being typed into a file the plug-in is reading: a
    /// threshold on its way from `0.9` to `0.95` passes through `0`,
    /// and for the length of a keystroke the gate is wide open. So a
    /// value settles when the user does something else, and at the
    /// latest when the window closes.
    ///
    /// Text that is not yet a value of the right shape is kept in the
    /// box and out of the file: half a number is not a number, and
    /// writing `1` for a half-typed `1.5` would be worse than waiting.
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

    /// The same deferral for the boxes inside a repeating group.
    ///
    /// These were the one place a keystroke still reached the file: a
    /// card's box is addressed by row and field where a control is
    /// addressed by an index, and the caller used to know only the
    /// latter — so every keystroke settled the *previous* keystroke, and
    /// a message on its way to being written arrived in the plug-in's
    /// config one prefix at a time. [`Typing`] says which box, whichever
    /// kind it is, and the answer is the same for both.
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
    /// Empty members are dropped rather than written, so a trailing
    /// comma while typing does not put `""` in the list — which, for
    /// the substring matching these lists usually feed, would match
    /// everything.
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
    /// is not running, one typed in by hand — and clearing what cannot
    /// be seen is the worse surprise of the two: the user is acting on a
    /// list they are looking at.
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
    /// The one place the file is written, which is also what makes it
    /// the one place the cached arrays have to be brought back in step
    /// — a ticked box that re-read nothing would spring back open on
    /// the next frame.
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
    /// plug-in may be running and watching that file, and a pane that
    /// held changes back would show a state the plug-in is not in.
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
