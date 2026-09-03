#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use poltertype_core::plugins::{DiscoveredExtension, ExtensionManifest, PluginCommand, TrayItem};
use tray_icon::menu::{Menu, MenuItem};

use super::refresh::count_label;
use super::rows::parse_rows;
use super::state::PluginMenu;

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
                    ..TrayItem::default()
                })
                .collect(),
            ..ExtensionManifest::default()
        },
        development: true,
    }
}

// The five tests below build a real `muda::Menu`: what is worth
// testing is which command a click resolves to, and a fake menu would
// only re-assert our own mock. On macOS that construction calls into
// AppKit, which refuses off the main thread — where `cargo test` never
// runs a `#[test]`. `#[ignore]`d there rather than faked, because
// ignoring is honest about not being exercised. The same build+route
// logic runs for real every time the tray menu is drawn.

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "muda::Menu can only be constructed on the main thread (AppKit); cargo test never runs a test there"
)]
fn a_plugin_with_no_entries_adds_nothing() {
    // The tray belongs to the user; a plug-in earns space in it by
    // having something to put there.
    let menu = Menu::new();
    let plugins = PluginMenu::build(vec![extension("quiet", &[])], &menu).unwrap();
    assert!(plugins.routes.is_empty());
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "muda::Menu can only be constructed on the main thread (AppKit); cargo test never runs a test there"
)]
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
#[cfg_attr(
    target_os = "macos",
    ignore = "muda::Menu can only be constructed on the main thread (AppKit); cargo test never runs a test there"
)]
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
#[cfg_attr(
    target_os = "macos",
    ignore = "muda::Menu can only be constructed on the main thread (AppKit); cargo test never runs a test there"
)]
fn a_click_on_something_else_is_not_claimed() {
    // The app's own menu items must fall through untouched.
    let menu = Menu::new();
    let mut plugins = PluginMenu::build(vec![extension("demo", &[("Go", "go")])], &menu).unwrap();
    let ours = MenuItem::new("Quit", true, None);
    assert!(!plugins.handle(ours.id()));
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "muda::Menu can only be constructed on the main thread (AppKit); cargo test never runs a test there"
)]
fn a_click_on_a_plugin_entry_is_claimed() {
    let menu = Menu::new();
    let mut plugins = PluginMenu::build(vec![extension("demo", &[("Go", "go")])], &menu).unwrap();
    let id = plugins.routes.keys().next().unwrap().clone();
    assert!(plugins.handle(&id));
}

// ── Showing which mode is in force ──────────────────────────────────

fn state(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn tray(label: &str, command: &str, key: &str, value: &str) -> TrayItem {
    TrayItem {
        label: label.to_owned(),
        command: command.to_owned(),
        state_key: key.to_owned(),
        state_value: value.to_owned(),
    }
}

#[test]
fn an_entry_without_state_is_still_a_plain_command() {
    // The shape every existing plug-in uses must not change meaning.
    let item = tray("Draft a reply", "propose", "", "");
    assert!(!item.is_check());
    assert!(!item.is_status());
    assert_eq!(item.render(Some(&state(&[]))), "Draft a reply");
}

#[test]
fn an_entry_with_a_key_and_a_value_is_a_tick() {
    let item = tray("Ask before acting", "mode-ask", "mode", "ask");
    assert!(item.is_check());
    assert!(!item.is_status());
    // The label carries the tick as a character too: a native
    // checkmark is drawn differently, faintly, or not at all depending
    // on the tray backend, and an indicator nobody can see is the bug
    // this mechanism exists to fix.
    assert_eq!(
        item.render(Some(&state(&[("mode", "ask")]))),
        "✓ Ask before acting"
    );
    assert_eq!(
        item.render(Some(&state(&[("mode", "learn")]))),
        "\u{2007} Ask before acting",
        "an inactive alternative is padded, so nothing shifts sideways"
    );
}

#[test]
fn an_entry_with_a_key_and_no_value_reports_rather_than_acts() {
    let item = tray("Autopilot — {}", "", "mode", "");
    assert!(item.is_status());
    assert!(!item.is_check());
    assert_eq!(
        item.render(Some(&state(&[("mode", "ask")]))),
        "Autopilot — ask"
    );
}

#[test]
fn a_status_label_without_a_placeholder_still_shows_the_value() {
    // A plug-in author who forgets `{}` should not get a line that
    // silently drops the one thing it exists to say.
    let item = tray("Autopilot", "", "mode", "");
    assert_eq!(
        item.render(Some(&state(&[("mode", "auto")]))),
        "Autopilot: auto"
    );
}

#[test]
fn an_unreported_key_says_unknown_rather_than_going_blank() {
    // The plug-in may be stopped, or its state command may have failed.
    // "unknown" is information; an empty gap is a puzzle.
    let item = tray("Autopilot — {}", "", "mode", "");
    assert_eq!(item.render(Some(&state(&[]))), "Autopilot — unknown");
    assert_eq!(
        item.render(Some(&state(&[("something_else", "x")]))),
        "Autopilot — unknown"
    );
}

#[test]
fn only_the_matching_alternative_is_ticked() {
    let items = [
        tray("Ask", "mode-ask", "mode", "ask"),
        tray("Learn", "mode-learn", "mode", "learn"),
        tray("Stop", "mode-off", "mode", "off"),
    ];
    let live = state(&[("mode", "learn")]);
    let ticked: Vec<bool> = items
        .iter()
        .map(|i| live.get(&i.state_key).is_some_and(|v| *v == i.state_value))
        .collect();
    assert_eq!(ticked, vec![false, true, false]);
}

#[test]
fn a_mode_no_entry_offers_ticks_nothing_rather_than_guessing() {
    // `auto` is reachable from the command line but deliberately has no
    // tray entry — arming unattended authority from a menu is too easy.
    // The menu must then show no tick at all, not the nearest one.
    let items = [
        tray("Ask", "mode-ask", "mode", "ask"),
        tray("Learn", "mode-learn", "mode", "learn"),
    ];
    let live = state(&[("mode", "auto")]);
    assert!(
        !items
            .iter()
            .any(|i| live.get(&i.state_key).is_some_and(|v| *v == i.state_value))
    );
}

#[test]
fn a_plugin_that_cannot_be_asked_says_so_rather_than_unknown() {
    // "unknown" means it answered and did not mention this key —
    // ordinary. A plug-in that could not be run at all is something to
    // go and look at, and the two must not read the same.
    let item = tray("Autopilot — {}", "", "mode", "");
    assert_eq!(item.render(None), "Autopilot — not responding");
    assert_eq!(item.render(Some(&state(&[]))), "Autopilot — unknown");
}

#[test]
fn nothing_is_active_when_the_plugin_could_not_be_asked() {
    let item = tray("Ask", "mode-ask", "mode", "ask");
    assert!(!item.is_active(None));
    assert!(item.is_active(Some(&state(&[("mode", "ask")]))));
}

#[test]
fn a_row_is_an_id_a_label_and_as_much_detail_as_it_prints() {
    let rows = parse_rows(
        "43\tElement · Піккатцо · 20m\treplying to: Лєший\t«То що це за айпейс?»\n\
         44\tWhatsApp · Котовод · 20m\n\
         \n\
         45\n",
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id, "43");
    assert_eq!(rows[0].label, "Element · Піккатцо · 20m");
    assert_eq!(
        rows[0].details,
        vec!["replying to: Лєший", "«То що це за айпейс?»"]
    );
    // A row may carry nothing but a label…
    assert!(rows[1].details.is_empty());
    // …or nothing but an id, which then has to stand as its own label:
    // an unlabelled entry is unclickable in practice.
    assert_eq!(rows[2].label, "45");
}

#[test]
fn a_row_without_an_id_is_dropped_rather_than_shown() {
    // There would be nothing to hand back to the plug-in, so the entry
    // could only ever act on the wrong thing or on nothing.
    assert!(parse_rows("\tno id here\tdetail").is_empty());
}

#[test]
fn the_count_reaches_the_title_with_or_without_a_placeholder() {
    assert_eq!(count_label("Drafts waiting ({})", 3), "Drafts waiting (3)");
    // No placeholder: appended rather than dropped, because the whole
    // point of the count is not having to open the menu to see it.
    assert_eq!(count_label("Drafts waiting", 0), "Drafts waiting (0)");
}
