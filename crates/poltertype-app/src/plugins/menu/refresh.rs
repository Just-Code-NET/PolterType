//! Re-reading plug-in state and redrawing the entries that show it.

use std::collections::HashMap;

use tray_icon::menu::{MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::plugins::supervisor::{read_rows, read_state};

use super::enums::StateItem;
use super::state::PluginMenu;
use super::types::RowRoute;

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
                StateItem::Check { item, spec } => {
                    item.set_checked(spec.is_active(state));
                    item.set_text(spec.render(state));
                }
                StateItem::Status { item, spec } => {
                    item.set_text(spec.render(state));
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

    /// Throw away every runtime menu's contents and build them again
    /// from what the plug-in prints now.
    ///
    /// Rebuilt whole rather than diffed: a menu that kept the items it
    /// recognised would have to decide what "the same row" means, and
    /// getting that wrong acts on the row above the one pointed at.
    fn refresh_lists(&mut self) {
        if self.lists.is_empty() {
            return;
        }
        self.row_routes.clear();
        // Collected first so the borrow of `self.extensions` ends before
        // the routes are written back.
        let mut built: Vec<Vec<(MenuId, RowRoute)>> = Vec::new();

        for list in &self.lists {
            let Some(ext) = self.extensions.get(list.ext) else {
                continue;
            };
            let rows = read_rows(ext, &list.spec.command);
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
                continue;
            }
            list.root
                .set_text(count_label(&list.spec.label, rows.len()));
            list.root.set_enabled(true);

            let mut routes = Vec::new();
            for row in &rows {
                // Each row is a submenu of its own: the label is all a
                // menu row has space for; the detail waits one hover away.
                let entry = Submenu::new(&row.label, true);
                for detail in &row.details {
                    let line = MenuItem::new(detail, false, None);
                    let _ = entry.append(&line);
                }
                if !row.details.is_empty() && !list.spec.actions.is_empty() {
                    let _ = entry.append(&PredefinedMenuItem::separator());
                }
                for action in &list.spec.actions {
                    let item = MenuItem::new(&action.label, true, None);
                    routes.push((
                        item.id().clone(),
                        (list.ext, action.command.clone(), row.id.clone()),
                    ));
                    let _ = entry.append(&item);
                }
                let _ = list.root.append(&entry);
            }
            if !list.spec.bulk.is_empty() {
                let _ = list.root.append(&PredefinedMenuItem::separator());
                for action in &list.spec.bulk {
                    let item = MenuItem::new(&action.label, true, None);
                    routes.push((
                        item.id().clone(),
                        (list.ext, action.command.clone(), String::new()),
                    ));
                    let _ = list.root.append(&item);
                }
            }
            built.push(routes);
        }

        for routes in built {
            self.row_routes.extend(routes);
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
