//! A list control's array: the members currently on disk, and adding
//! or removing one (or all of them).

use tracing::warn;

use poltertype_core::i18n::tr_args;
use poltertype_core::plugins::{ControlKind, read_string_array};

use super::pane::PluginPane;
use super::types::Slot;

impl PluginPane {
    /// Re-read every list control's array from the plug-in's config.
    ///
    /// One read and one parse per list control, on a step the user took
    /// — not per row and not per frame. Another program owns this file,
    /// so the answer still comes from disk rather than from what this
    /// pane last wrote.
    pub(super) fn reload_arrays(&mut self) {
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
                self.status = Some(tr_args(
                    "plugins.status_change_failed",
                    "Could not change {}: {}",
                    &[&key, &e.to_string()],
                ));
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
                self.status = Some(tr_args(
                    "plugins.status_change_failed",
                    "Could not change {}: {}",
                    &[&key, &e.to_string()],
                ));
            }
        }
    }
}
