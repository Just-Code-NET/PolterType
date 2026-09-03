//! Boxes fed by a plug-in command: which ones need asking, what they
//! are showing, and the suggestion lists built from the answer.

use poltertype_core::plugins::{ControlKind, SettingValue};

use super::enums::CommandOutput;
use super::helpers::parse_list_rows;
use super::pane::PluginPane;
use super::types::{ListRow, Slot};

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
}
