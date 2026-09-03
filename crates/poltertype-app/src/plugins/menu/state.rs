//! `PluginMenu` — the plug-in half of the tray menu, and its fields.

use std::collections::HashMap;

use anyhow::{Context, Result};
use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use super::enums::StateItem;
use super::types::{ListMenu, RowRoute};

/// The plug-in half of the tray menu: the entries, and what they mean.
pub struct PluginMenu {
    pub(super) extensions: Vec<DiscoveredExtension>,
    /// Menu item id → (index into `extensions`, command id).
    pub(super) routes: HashMap<MenuId, (usize, String)>,
    /// Per extension index, the entries that reflect its state.
    pub(super) stateful: Vec<(usize, StateItem)>,
    /// Runtime menus, and their routes — kept apart from `routes`
    /// because every refresh throws these away and builds new ones with
    /// new ids, while the manifest's own entries live as long as the
    /// menu does.
    pub(super) lists: Vec<ListMenu>,
    pub(super) row_routes: HashMap<MenuId, RowRoute>,
    /// How many things the plug-ins are waiting on the owner for, summed
    /// over those that declared a key for it. Read by the tray icon.
    pub(super) attention: u32,
}

impl PluginMenu {
    /// Append one section per plug-in that declares menu entries.
    ///
    /// A plug-in with nothing to contribute adds nothing — no empty
    /// section, no separator, no evidence it is there.
    pub fn build(mut extensions: Vec<DiscoveredExtension>, menu: &Menu) -> Result<Self> {
        // Its entries are the plug-in's own words, so they come from
        // the plug-in's own catalog — the same substitution the settings
        // pane does, before a single label is read out of the manifest.
        for ext in &mut extensions {
            poltertype_core::plugins::localise(&mut ext.manifest, &ext.id);
        }

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
                    // Disabled: it reports, it does not act.
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
            // Runtime menus last, at the bottom of the plug-in's own
            // block rather than between two of its settings.
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
        // Start truthful rather than blank: nothing ticked reads as "no
        // mode is set", when in fact one always is.
        this.refresh();
        Ok(this)
    }

    /// How many things the plug-ins are waiting on the owner for.
    pub fn attention(&self) -> u32 {
        self.attention
    }

    /// Does any plug-in report state worth re-reading? Decides whether
    /// the heartbeat runs at all.
    pub fn reports_state(&self) -> bool {
        !self.stateful.is_empty() || !self.lists.is_empty()
    }

    pub fn extensions(&self) -> &[DiscoveredExtension] {
        &self.extensions
    }
}
