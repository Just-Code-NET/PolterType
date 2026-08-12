//! Plug-in entries in the tray menu.
//!
//! A plug-in declares menu entries in its manifest; this turns them
//! into real items and remembers which item means which command. The
//! app's own menu handling stays a list of `if id == …` comparisons
//! and asks this module one question at the end: "was that one of the
//! plug-ins'?"
//!
//! Routing by the item's own id — never by label or position — keeps
//! two plug-ins that both call an entry "Settings…" apart, and keeps
//! either from matching one of ours.
//!
//! An entry may declare which key of the plug-in's state it reflects,
//! and the menu is refreshed from the plug-in itself, never from its
//! config file, which holds only what it *starts* as. The live value is
//! shown twice on purpose — a **tick** on the active alternative and a
//! **status line** naming it in words — because a tick is small, drawn
//! differently by every tray backend, and sometimes not drawn at all.
//! One of the two is redundant on any given desktop and which one is
//! not knowable from here.

use std::collections::HashMap;

use anyhow::{Context, Result};
use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use super::supervisor::{read_rows, read_state, run_command, run_command_for_row};

/// One entry of a plug-in's runtime menu, as the plug-in printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRow {
    /// Handed back to the plug-in when an action on this row is chosen.
    pub id: String,
    /// The line the user reads in the menu.
    pub label: String,
    /// Lines shown under it, disabled. This is where a row says what it
    /// actually holds — who wrote, what the reply would be — without a
    /// window having to be opened to find out.
    pub details: Vec<String>,
}

/// Parse a list command's output into rows: `id`, label, then any number
/// of detail lines, tab-separated.
///
/// The same shape the settings pane's tick-box lists use, and tolerant in
/// the same way: a line with no tab is an id that is its own label, blank
/// lines are skipped, and a row with no id is dropped because there would
/// be nothing to act on.
pub fn parse_rows(text: &str) -> Vec<MenuRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next().unwrap_or_default().trim().to_owned();
            let label = fields.next().unwrap_or_default().trim().to_owned();
            let details = fields
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_owned)
                .collect();
            MenuRow {
                label: if label.is_empty() { id.clone() } else { label },
                id,
                details,
            }
        })
        .filter(|row| !row.id.is_empty())
        .collect()
}

/// A menu entry that mirrors plug-in state, and how to redraw it.
enum StateItem {
    /// Ticked when the reported value matches.
    Check {
        item: CheckMenuItem,
        /// Kept whole: the label carries a glyph that has to be
        /// re-rendered whenever the live alternative changes.
        spec: poltertype_core::plugins::TrayItem,
    },
    /// A disabled line naming the current value.
    Status {
        item: MenuItem,
        /// Kept whole rather than as a rendered string: the label is a
        /// template and has to be re-rendered on every refresh.
        spec: poltertype_core::plugins::TrayItem,
    },
}

/// What a runtime menu entry does when it is clicked: which plug-in,
/// which of its commands, and which row (empty for an action on the whole
/// list).
type RowRoute = (usize, String, String);

/// A submenu whose contents come from the plug-in each time state is
/// read, rather than from the manifest.
struct ListMenu {
    /// Index into `extensions`.
    ext: usize,
    spec: poltertype_core::plugins::TrayList,
    /// The submenu itself, which stays put in the tray menu; only its
    /// contents are replaced. Removing and re-adding the submenu would
    /// move it around the menu as the list filled and emptied.
    root: Submenu,
}

/// The plug-in half of the tray menu: the entries, and what they mean.
pub struct PluginMenu {
    extensions: Vec<DiscoveredExtension>,
    /// Menu item id → (index into `extensions`, command id).
    routes: HashMap<MenuId, (usize, String)>,
    /// Per extension index, the entries that reflect its state.
    stateful: Vec<(usize, StateItem)>,
    /// Runtime menus, and their routes — kept apart from `routes`
    /// because every refresh throws these away and builds new ones with
    /// new ids, while the manifest's own entries live as long as the
    /// menu does.
    lists: Vec<ListMenu>,
    row_routes: HashMap<MenuId, RowRoute>,
    /// How many things the plug-ins are waiting on the owner for, summed
    /// over those that declared a key for it. Read by the tray icon.
    attention: u32,
}

impl PluginMenu {
    /// Append one section per plug-in that declares menu entries.
    ///
    /// A plug-in with nothing to contribute adds nothing — no empty
    /// section, no separator, no evidence it is there. The tray belongs
    /// to the user.
    pub fn build(extensions: Vec<DiscoveredExtension>, menu: &Menu) -> Result<Self> {
        let mut routes = HashMap::new();
        let mut stateful: Vec<(usize, StateItem)> = Vec::new();
        let mut lists: Vec<ListMenu> = Vec::new();
        let mut keep: Vec<MenuItem> = Vec::new();

        for (index, ext) in extensions.iter().enumerate() {
            if ext.manifest.tray_items.is_empty() && ext.manifest.tray_lists.is_empty() {
                continue;
            }
            menu.append(&PredefinedMenuItem::separator())
                .context("separator before plug-in menu entries")?;

            for entry in &ext.manifest.tray_items {
                if entry.is_status() {
                    // Disabled: it reports, it does not act. Clicking it
                    // should do nothing, and looking disabled is how a
                    // menu says so before you try.
                    let item = MenuItem::new(entry.render(None), false, None);
                    menu.append(&item)
                        .with_context(|| format!("plug-in status entry {:?}", entry.label))?;
                    stateful.push((
                        index,
                        StateItem::Status {
                            item,
                            spec: entry.clone(),
                        },
                    ));
                    continue;
                }

                if entry.is_check() {
                    let item = CheckMenuItem::new(entry.render(None), true, false, None);
                    routes.insert(item.id().clone(), (index, entry.command.clone()));
                    menu.append(&item)
                        .with_context(|| format!("plug-in menu entry {:?}", entry.label))?;
                    stateful.push((
                        index,
                        StateItem::Check {
                            item,
                            spec: entry.clone(),
                        },
                    ));
                    continue;
                }

                let item = MenuItem::new(&entry.label, true, None);
                routes.insert(item.id().clone(), (index, entry.command.clone()));
                menu.append(&item)
                    .with_context(|| format!("plug-in menu entry {:?}", entry.label))?;
                // The menu holds a clone internally, but the item must
                // outlive the borrow used to append it.
                keep.push(item);
            }
            // The runtime menus come last, under the fixed entries: what
            // is waiting is what the owner came to the tray for, and it
            // reads better at the bottom of the plug-in's own block than
            // between two settings.
            for spec in &ext.manifest.tray_lists {
                if spec.command.trim().is_empty() {
                    warn!(id = %ext.id, label = %spec.label, "tray list names no command — skipped");
                    continue;
                }
                let root = Submenu::new(&spec.label, false);
                menu.append(&root)
                    .with_context(|| format!("plug-in menu list {:?}", spec.label))?;
                lists.push(ListMenu {
                    ext: index,
                    spec: spec.clone(),
                    root,
                });
            }

            info!(
                id = %ext.id,
                entries = ext.manifest.tray_items.len(),
                lists = ext.manifest.tray_lists.len(),
                "plug-in contributed tray entries"
            );
        }

        drop(keep);
        let mut this = Self {
            extensions,
            routes,
            stateful,
            lists,
            row_routes: HashMap::new(),
            attention: 0,
        };
        // Start truthful rather than blank: without this the first look
        // at the menu shows nothing ticked, which reads as "no mode is
        // set" when in fact one always is.
        this.refresh();
        Ok(this)
    }

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

        // The mark on the icon is counted from the same state read the
        // entries used, so the number on the icon and the number in the
        // menu can never disagree.
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
    /// Rebuilt whole rather than diffed: the rows are a queue, the ids
    /// are the plug-in's, and a menu that tried to keep the items it
    /// recognised would have to decide what "the same row" means. It is
    /// a handful of items behind a click, and getting it wrong means
    /// acting on the row above the one that was pointed at.
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
                // Disabled, so a menu with nothing in it cannot be
                // opened onto a blank rectangle.
                list.root.set_enabled(false);
                continue;
            }
            list.root
                .set_text(count_label(&list.spec.label, rows.len()));
            list.root.set_enabled(true);

            let mut routes = Vec::new();
            for row in &rows {
                // Each row is a submenu of its own: the label is what
                // there is room for in a menu, and everything the row
                // actually holds waits one hover away.
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

    /// How many things the plug-ins are waiting on the owner for.
    pub fn attention(&self) -> u32 {
        self.attention
    }

    /// Handle a menu click if it belongs to a plug-in. Returns whether
    /// it did, so the caller can stop looking.
    pub fn handle(&mut self, id: &MenuId) -> bool {
        if let Some((index, command, row)) = self.row_routes.get(id).cloned() {
            let Some(ext) = self.extensions.get(index) else {
                return false;
            };
            let outcome = if row.is_empty() {
                run_command(ext, &command)
            } else {
                run_command_for_row(ext, &command, &row)
            };
            if let Err(e) = outcome {
                warn!(id = %ext.id, "plug-in list entry failed: {e}");
            }
            // Acting on a row usually removes it, and a stale row is
            // worse here than a stale tick: clicking it again would act
            // on something that is gone.
            std::thread::sleep(REFRESH_SETTLE);
            self.refresh();
            return true;
        }
        let Some((index, command)) = self.routes.get(id).cloned() else {
            return false;
        };
        let Some(ext) = self.extensions.get(index) else {
            return false;
        };
        if let Err(e) = run_command(ext, &command) {
            warn!(id = %ext.id, "plug-in menu entry failed: {e}");
        }

        // The click almost certainly changed what the menu should show,
        // and this is the one moment we know to look. A command is
        // spawned rather than waited on, though, so the state it sets
        // may not have landed yet — hence the settle below, and hence
        // `refresh` staying public for the periodic caller.
        std::thread::sleep(REFRESH_SETTLE);
        self.refresh();
        true
    }

    /// Does any plug-in report state worth re-reading? The caller uses
    /// this to decide whether to run a heartbeat at all — an app that
    /// wakes on a timer to do nothing is worse than one that sleeps.
    pub fn reports_state(&self) -> bool {
        !self.stateful.is_empty() || !self.lists.is_empty()
    }

    pub fn extensions(&self) -> &[DiscoveredExtension] {
        &self.extensions
    }
}

/// How long to let a just-launched command finish before re-reading
/// state.
///
/// A menu click spawns the command without waiting, so reading back
/// immediately races it and shows the value the user just replaced.
/// Bounded to something nobody perceives as a hang, since this is on
/// the UI thread; the periodic refresh corrects a slower command anyway.
const REFRESH_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

/// Empty a submenu, keeping the submenu itself where it is.
fn clear_submenu(menu: &Submenu) {
    while menu.remove_at(0).is_some() {}
}

/// A list's title with `{}` replaced by how many rows are in it. Without
/// a placeholder the count is appended, because "Drafts waiting" and
/// "Drafts waiting (3)" are different sentences and only one of them
/// saves opening the menu.
fn count_label(label: &str, rows: usize) -> String {
    if label.contains("{}") {
        label.replacen("{}", &rows.to_string(), 1)
    } else {
        format!("{label} ({rows})")
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
