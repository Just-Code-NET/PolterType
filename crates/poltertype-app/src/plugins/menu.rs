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

use std::collections::HashMap;

use anyhow::{Context, Result};
use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};
use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};

use super::supervisor::run_command;

/// The plug-in half of the tray menu: the entries, and what they mean.
pub struct PluginMenu {
    extensions: Vec<DiscoveredExtension>,
    /// Menu item id → (index into `extensions`, command id).
    routes: HashMap<MenuId, (usize, String)>,
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
        let mut items: Vec<MenuItem> = Vec::new();

        for (index, ext) in extensions.iter().enumerate() {
            if ext.manifest.tray_items.is_empty() {
                continue;
            }
            menu.append(&PredefinedMenuItem::separator())
                .context("separator before plug-in menu entries")?;
            for entry in &ext.manifest.tray_items {
                let item = MenuItem::new(&entry.label, true, None);
                routes.insert(item.id().clone(), (index, entry.command.clone()));
                menu.append(&item)
                    .with_context(|| format!("plug-in menu entry {:?}", entry.label))?;
                // The menu holds a clone internally, but the item must
                // outlive the borrow used to append it.
                items.push(item);
            }
            info!(
                id = %ext.id,
                entries = ext.manifest.tray_items.len(),
                "plug-in contributed tray entries"
            );
        }

        // Items are kept alive by the menu itself; ours were only
        // needed for their ids.
        drop(items);
        Ok(Self { extensions, routes })
    }

    /// Handle a menu click if it belongs to a plug-in. Returns whether
    /// it did, so the caller can stop looking.
    pub fn handle(&self, id: &MenuId) -> bool {
        let Some((index, command)) = self.routes.get(id) else {
            return false;
        };
        let Some(ext) = self.extensions.get(*index) else {
            return false;
        };
        if let Err(e) = run_command(ext, command) {
            warn!(id = %ext.id, "plug-in menu entry failed: {e}");
        }
        true
    }

    pub fn extensions(&self) -> &[DiscoveredExtension] {
        &self.extensions
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
