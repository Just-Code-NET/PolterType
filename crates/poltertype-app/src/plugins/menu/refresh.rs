//! Re-reading plug-in state and redrawing the entries that show it.

use std::collections::HashMap;

use tray_icon::menu::{MenuItem, PredefinedMenuItem, Submenu};

use crate::plugins::supervisor::{read_rows, read_state};

use super::enums::{ListEntry, StateItem};
use super::rows::list_entries;
use super::state::PluginMenu;

impl PluginMenu {
    /// Re-read every plug-in's state and redraw the entries that show
    /// it. No subprocess runs for a plug-in that reports none, and the
    /// whole pass is skipped when no entry would change.
    pub fn refresh(&mut self) {
        if self.stateful.is_empty() && self.lists.is_empty() {
            return;
        }
        let mut cache: HashMap<usize, Option<HashMap<String, String>>> = HashMap::new();

        for (index, entry) in &self.stateful {
            let Some(ext) = self.extensions.get(*index) else {
                continue;
            };
            let state = cache
                .entry(*index)
                .or_insert_with(|| read_state(ext))
                .clone();
            let state = state.as_ref();

            match entry {
                // Compared first: writing an unchanged value still tells
                // a desktop's tray host the menu changed.
                StateItem::Check { item, spec } => {
                    let active = spec.is_active(state);
                    if item.is_checked() != active {
                        item.set_checked(active);
                    }
                    let text = spec.render(state);
                    if item.text() != text {
                        item.set_text(text);
                    }
                }
                StateItem::Status { item, spec } => {
                    let text = spec.render(state);
                    if item.text() != text {
                        item.set_text(text);
                    }
                }
            }
        }

        self.refresh_lists();

        // Counted from the same state read the entries used, so the icon
        // and the menu can never disagree.
        self.attention = self
            .extensions
            .iter()
            .enumerate()
            .filter(|(_, ext)| !ext.manifest.attention_state_key.trim().is_empty())
            .filter_map(|(index, ext)| {
                cache
                    .entry(index)
                    .or_insert_with(|| read_state(ext))
                    .as_ref()
                    .and_then(|s| s.get(ext.manifest.attention_state_key.trim()))
                    .and_then(|v| v.trim().parse::<u32>().ok())
            })
            .sum();
    }

    /// Rebuild the runtime menus whose rows have changed.
    ///
    /// **Only those, because a rebuild takes the menu away from under the
    /// pointer.** Removing an open submenu's items closes it, and this
    /// runs on a heartbeat: rebuilding every time threw whoever was
    /// reading a draft back to the top of the tray every fifteen seconds.
    /// A plug-in that could not be asked leaves its list as it stands, for
    /// the same reason — one slow answer is not an emptied queue.
    ///
    /// Rebuilt whole rather than diffed: a menu that kept the items it
    /// recognised would have to decide what "the same row" means, and
    /// getting that wrong acts on the row above the one pointed at.
    fn refresh_lists(&mut self) {
        for list in &mut self.lists {
            let Some(ext) = self.extensions.get(list.ext) else {
                continue;
            };
            let Some(rows) = read_rows(ext, &list.spec.command) else {
                continue;
            };
            if list.shown.as_ref() == Some(&rows) {
                continue;
            }
            for id in list.route_ids.drain(..) {
                self.row_routes.remove(&id);
            }
            clear_submenu(&list.root);

            if rows.is_empty() {
                let empty = list.spec.empty_label.trim();
                list.root.set_text(if empty.is_empty() {
                    count_label(&list.spec.label, 0)
                } else {
                    empty.to_owned()
                });
                // Disabled, so it cannot open onto a blank rectangle.
                list.root.set_enabled(false);
                list.shown = Some(rows);
                continue;
            }
            list.root
                .set_text(count_label(&list.spec.label, rows.len()));
            list.root.set_enabled(true);

            for entry in list_entries(&rows, &list.spec) {
                match entry {
                    ListEntry::Note(text) => {
                        let _ = list.root.append(&MenuItem::new(text, false, None));
                    }
                    ListEntry::Separator => {
                        let _ = list.root.append(&PredefinedMenuItem::separator());
                    }
                    ListEntry::Action {
                        label,
                        command,
                        row,
                    } => {
                        let item = MenuItem::new(label, true, None);
                        list.route_ids.push(item.id().clone());
                        self.row_routes
                            .insert(item.id().clone(), (list.ext, command, row));
                        let _ = list.root.append(&item);
                    }
                }
            }
            list.shown = Some(rows);
        }
    }
}

/// Empty a submenu, keeping the submenu itself where it is.
fn clear_submenu(menu: &Submenu) {
    while menu.remove_at(0).is_some() {}
}

/// A list's title with `{}` replaced by how many rows are in it. Without
/// a placeholder the count is appended, because "Drafts waiting" and
/// "Drafts waiting (3)" are different sentences and only one of them
/// saves opening the menu.
pub(super) fn count_label(label: &str, rows: usize) -> String {
    if label.contains("{}") {
        label.replacen("{}", &rows.to_string(), 1)
    } else {
        format!("{label} ({rows})")
    }
}
