//! A plain control's value: what is being typed, settling it into the
//! plug-in's config file, and writing that file back.

use tracing::warn;

use poltertype_core::plugins::{ControlKind, SettingValue, write_setting, write_string_array};

use super::enums::Typing;
use super::pane::PluginPane;

impl PluginPane {
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

    /// Write the plug-in's config file back, reporting either way, and
    /// say whether it landed.
    ///
    /// The one place the file is written, so also the one place the
    /// cached arrays are brought back in step — a ticked box that
    /// re-read nothing springs back open on the next frame.
    pub(super) fn write(&mut self, updated: String) -> bool {
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
