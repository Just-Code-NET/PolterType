#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use poltertype_core::plugins::{DiscoveredExtension, ExtensionManifest, PluginCommand, TrayItem};

use super::*;

fn extension(id: &str, entries: &[(&str, &str)]) -> DiscoveredExtension {
    DiscoveredExtension {
        id: id.to_owned(),
        name: id.to_owned(),
        version: "0".to_owned(),
        dir: std::env::temp_dir(),
        exe: PathBuf::from("/bin/true"),
        manifest: ExtensionManifest {
            exe: "true".to_owned(),
            commands: entries
                .iter()
                .map(|(_, command)| PluginCommand {
                    id: (*command).to_owned(),
                    label: (*command).to_owned(),
                    args: Vec::new(),
                })
                .collect(),
            tray_items: entries
                .iter()
                .map(|(label, command)| TrayItem {
                    label: (*label).to_owned(),
                    command: (*command).to_owned(),
                })
                .collect(),
            ..ExtensionManifest::default()
        },
        development: true,
    }
}

#[test]
fn a_plugin_with_no_entries_adds_nothing() {
    // The tray belongs to the user; a plug-in earns space in it by
    // having something to put there.
    let menu = Menu::new();
    let plugins = PluginMenu::build(vec![extension("quiet", &[])], &menu).unwrap();
    assert!(plugins.routes.is_empty());
}

#[test]
fn each_entry_routes_to_its_own_command() {
    let menu = Menu::new();
    let plugins = PluginMenu::build(
        vec![extension("demo", &[("Do A", "a"), ("Do B", "b")])],
        &menu,
    )
    .unwrap();

    assert_eq!(plugins.routes.len(), 2);
    let commands: Vec<&str> = plugins
        .routes
        .values()
        .map(|(_, c)| c.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(commands, vec!["a", "b"]);
}

#[test]
fn two_plugins_with_identical_labels_stay_distinct() {
    // Routing by menu id rather than by label is what keeps these
    // apart — and keeps either from ever matching one of ours.
    let menu = Menu::new();
    let plugins = PluginMenu::build(
        vec![
            extension("first", &[("Settings…", "one")]),
            extension("second", &[("Settings…", "two")]),
        ],
        &menu,
    )
    .unwrap();

    assert_eq!(plugins.routes.len(), 2);
    let owners: std::collections::BTreeSet<usize> =
        plugins.routes.values().map(|(i, _)| *i).collect();
    assert_eq!(owners.len(), 2, "each entry must belong to its own plug-in");
}

#[test]
fn a_click_on_something_else_is_not_claimed() {
    // The app's own menu items must fall through untouched.
    let menu = Menu::new();
    let plugins = PluginMenu::build(vec![extension("demo", &[("Go", "go")])], &menu).unwrap();
    let ours = MenuItem::new("Quit", true, None);
    assert!(!plugins.handle(ours.id()));
}

#[test]
fn a_click_on_a_plugin_entry_is_claimed() {
    let menu = Menu::new();
    let plugins = PluginMenu::build(vec![extension("demo", &[("Go", "go")])], &menu).unwrap();
    let id = plugins.routes.keys().next().unwrap().clone();
    assert!(plugins.handle(&id));
}
