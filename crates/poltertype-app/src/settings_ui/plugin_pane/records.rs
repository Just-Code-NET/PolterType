//! Repeating-group rows: reading them back from the file, editing one
//! field, adding and removing cards, and the state behind a card's own
//! action button.

use poltertype_core::plugins::{ControlKind, SettingValue};

use super::pane::PluginPane;
use super::types::{RecordRow, Slot};

impl PluginPane {
    /// Re-read every repeating group from the file.
    ///
    /// Called wherever the file can have changed under us, like
    /// [`super::arrays`]'s reload: add, remove and set-field all rewrite
    /// the document, and a stale cache would draw the deleted row.
    pub(super) fn reload_records(&mut self) {
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

    /// Note what is being typed into a record's field. Written to the
    /// file by [`super::values`]'s flush, not here.
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
}
