//! Plug-in entries in the tray menu.
//!
//! A plug-in declares menu entries in its manifest; this turns them
//! into real items and remembers which item means which command. The
//! app's own menu handling stays a list of `if id == …` comparisons
//! against items it created itself, and asks this one question at the
//! end: "was that one of the plug-ins'?"
//!
//! Routing by the menu item's own id — rather than by label, or by
//! position — is what keeps two plug-ins that both call an entry
//! "Settings…" from being confused with each other, and keeps either
//! of them from ever matching one of ours.
//!
//! ## Showing what is in force
//!
//! A menu of alternatives that does not say which one is active is a
//! menu you have to guess at: you pick a mode and nothing on screen
//! changes, so you cannot tell whether the click landed, and you cannot
//! tell later what you left it set to. For a plug-in that decides how
//! much authority it has over the keyboard, that is the most important
//! thing on the screen.
//!
//! So an entry may declare which key of the plug-in's state it
//! reflects, and the menu is refreshed from the plug-in itself — never
//! from its config file, which holds only what it *starts* as. Both
//! renderings are used together, deliberately:
//!
//! * a **tick** on the live alternative, which is what a native menu
//!   is for; and
//! * a **status line** naming it in words, because a tick is small, is
//!   drawn differently by every tray backend, and is sometimes not
//!   drawn at all.
//!
//! One of those is redundant on any given desktop. Which one is
//! redundant is not knowable from here, which is the argument for
//! keeping both.

use std::collections::HashMap;

use anyhow::{Context, Result};
use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};

use super::supervisor::{read_state, run_command};

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

/// The plug-in half of the tray menu: the entries, and what they mean.
pub struct PluginMenu {
    extensions: Vec<DiscoveredExtension>,
    /// Menu item id → (index into `extensions`, command id).
    routes: HashMap<MenuId, (usize, String)>,
    /// Per extension index, the entries that reflect its state.
    stateful: Vec<(usize, StateItem)>,
}

impl PluginMenu {
    /// Append one section per plug-in that declares menu entries.
    ///
    /// A plug-in with nothing to contribute adds nothing — no empty
    /// section, no separator, no evidence it is there. The tray belongs
    /// to the user, and a plug-in earns space in it by having something
    /// to put there.
    pub fn build(extensions: Vec<DiscoveredExtension>, menu: &Menu) -> Result<Self> {
        let mut routes = HashMap::new();
        let mut stateful: Vec<(usize, StateItem)> = Vec::new();
        let mut keep: Vec<MenuItem> = Vec::new();

        for (index, ext) in extensions.iter().enumerate() {
            if ext.manifest.tray_items.is_empty() {
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
            info!(
                id = %ext.id,
                entries = ext.manifest.tray_items.len(),
                "plug-in contributed tray entries"
            );
        }

        drop(keep);
        let mut this = Self {
            extensions,
            routes,
            stateful,
        };
        // Start truthful rather than blank: without this the first look
        // at the menu shows nothing ticked, which reads as "no mode is
        // set" when in fact one always is.
        this.refresh();
        Ok(this)
    }

    /// Re-read every plug-in's state and redraw the entries that show
    /// it.
    ///
    /// Cheap when nothing declares state — no subprocess is run for a
    /// plug-in that reports none — and skipped entirely when no entry
    /// would change as a result.
    pub fn refresh(&mut self) {
        if self.stateful.is_empty() {
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
    }

    /// Handle a menu click if it belongs to a plug-in. Returns whether
    /// it did, so the caller can stop looking.
    pub fn handle(&mut self, id: &MenuId) -> bool {
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

    /// Does any plug-in report state worth re-reading?
    ///
    /// The caller uses this to decide whether to run a heartbeat at
    /// all: with no reporting plug-in there is nothing to refresh, and
    /// an app that wakes on a timer to do nothing is worse than one
    /// that sleeps.
    pub fn reports_state(&self) -> bool {
        !self.stateful.is_empty()
    }

    pub fn extensions(&self) -> &[DiscoveredExtension] {
        &self.extensions
    }
}

/// How long to let a just-launched command finish before re-reading
/// state.
///
/// A menu click spawns the command without waiting, so reading back
/// immediately races it and would show the value the user just replaced.
/// This is on the UI thread, so it is bounded to something no one
/// perceives as a hang — and the periodic refresh corrects it anyway if
/// the command was slower than this.
const REFRESH_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
