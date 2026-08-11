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
    /// What each command-backed control is showing, by control index.
    /// Absent means it has not been asked yet — which is why it is a
    /// map and not a vector of defaults: "never asked" and "asked,
    /// empty" are different, and only one of them should send a
    /// command.
    ///
    /// Private, and written only through [`Self::set_output`], so the
    /// rows parsed out of it cannot be left describing an older answer.
    outputs: std::collections::HashMap<usize, CommandOutput>,
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
    /// The rows a list control is drawing, parsed once when the
    /// plug-in's answer arrives rather than re-split on every rebuild.
    rows: std::collections::HashMap<usize, Vec<ListRow>>,
}

impl PluginPane {
    /// Which controls need a command run and have not had one yet.
    ///
    /// The pane asks on the way in rather than on every draw: each of
    /// these costs a process, and `view` is rebuilt on every state
    /// change — every click, every keystroke in a box. Only the
    /// section on screen is asked — reading a chat client's room list
    /// means talking to that application, and doing it for twelve
    /// sections nobody opened is a cost with nothing to show for it.
    pub fn unasked_commands(&self) -> Vec<usize> {
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                matches!(c.kind, ControlKind::Report | ControlKind::List)
                    && !self.outputs.contains_key(i)
                    && self.is_visible(*i)
            })
            .map(|(i, _)| i)
            .collect()
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
    pub fn unasked_by_command(&self) -> Vec<Vec<usize>> {
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for index in self.unasked_commands() {
            let Some(command) = self.control(index).map(|c| c.command.clone()) else {
                continue;
            };
            match groups.iter_mut().find(|(id, _)| *id == command) {
                Some((_, members)) => members.push(index),
                None => groups.push((command, vec![index])),
            }
        }
        groups.into_iter().map(|(_, members)| members).collect()
    }

    /// Every control fed by the same command as this one, itself
    /// included — what a Refresh should update, since they are all
    /// showing one answer.
    pub fn sharing_command(&self, index: usize) -> Vec<usize> {
        let Some(command) = self.control(index).map(|c| c.command.as_str()) else {
            return Vec::new();
        };
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                c.command == command
                    && matches!(c.kind, ControlKind::Report | ControlKind::List)
                    && self.is_visible(*i)
            })
            .map(|(i, _)| i)
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

    /// What a command-backed control is showing now.
    ///
    /// The one way to set an output, so the parsed rows cannot fall out
    /// of step with the text they came from.
    pub fn set_output(&mut self, index: usize, state: CommandOutput) {
        self.rows.remove(&index);
        if let CommandOutput::Ready(text) = &state {
            self.rows.insert(index, parse_list_rows(text));
        }
        self.outputs.insert(index, state);
    }

    /// What a command-backed control is showing, for the pane to draw.
    pub fn output(&self, index: usize) -> Option<&CommandOutput> {
        self.outputs.get(&index)
    }

    /// The rows of a list control: `id`, its label, and a line of detail.
    pub fn list_rows(&self, index: usize) -> &[ListRow] {
        self.rows.get(&index).map_or(&[], Vec::as_slice)
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
        };
        pane.reload_arrays();
        pane
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
    pub fn flush_edits(&mut self, still_typing: Option<usize>) {
        let pending: Vec<(usize, String)> = self
            .edits
            .iter()
            .filter(|(i, _)| Some(**i) != still_typing)
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
