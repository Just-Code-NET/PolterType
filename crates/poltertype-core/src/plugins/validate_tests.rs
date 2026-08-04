#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::plugins::enums::PluginKind;
use crate::plugins::types::{ExtensionManifest, PaneControl, PluginCommand, TrayItem};

fn command(id: &str) -> PluginCommand {
    PluginCommand {
        id: id.to_owned(),
        label: id.to_owned(),
        args: vec![id.to_owned()],
    }
}

fn base() -> ExtensionManifest {
    ExtensionManifest {
        exe: "some-plugin".to_owned(),
        commands: vec![command("run")],
        ..ExtensionManifest::default()
    }
}

#[test]
fn a_minimal_extension_is_accepted() {
    assert!(check_extension(&base()).is_ok());
}

#[test]
fn an_extension_must_name_a_program() {
    let m = ExtensionManifest {
        exe: "   ".to_owned(),
        ..base()
    };
    assert!(matches!(
        check_extension(&m),
        Err(PluginError::NoExecutable(_))
    ));
}

#[test]
fn the_program_may_not_be_a_path() {
    // The whole point: a manifest must not be able to point PolterType
    // at a binary somewhere else on the machine.
    for exe in [
        "/usr/bin/env",
        "../../../bin/sh",
        "sub/dir/tool",
        r"..\windows\system32\cmd.exe",
        "..",
        ".hidden",
    ] {
        let m = ExtensionManifest {
            exe: exe.to_owned(),
            ..base()
        };
        assert!(
            matches!(check_extension(&m), Err(PluginError::BadExecutablePath(_))),
            "{exe:?} should have been refused"
        );
    }
}

#[test]
fn a_tray_item_pointing_at_nothing_is_refused() {
    // A menu entry that silently does nothing is worse than no entry;
    // worse still, a later manifest could give that name a meaning.
    let m = ExtensionManifest {
        tray_items: vec![TrayItem {
            label: "Do the thing".to_owned(),
            command: "nonexistent".to_owned(),
            ..TrayItem::default()
        }],
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

#[test]
fn a_tray_item_pointing_at_a_real_command_is_fine() {
    let m = ExtensionManifest {
        tray_items: vec![TrayItem {
            label: "Run".to_owned(),
            command: "run".to_owned(),
            ..TrayItem::default()
        }],
        ..base()
    };
    assert!(check_extension(&m).is_ok());
}

#[test]
fn a_button_must_refer_to_a_real_command() {
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Button,
            label: "Go".to_owned(),
            command: "missing".to_owned(),
            ..PaneControl::default()
        }],
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

#[test]
fn a_stored_control_must_name_its_key() {
    for kind in [
        ControlKind::Toggle,
        ControlKind::Text,
        ControlKind::Number,
        ControlKind::Choice,
    ] {
        let m = ExtensionManifest {
            pane: vec![PaneControl {
                kind,
                key: String::new(),
                label: "Something".to_owned(),
                options: vec!["a".to_owned()],
                ..PaneControl::default()
            }],
            ..base()
        };
        assert!(
            matches!(check_extension(&m), Err(PluginError::BadPane(_))),
            "{kind:?} with no key should have been refused"
        );
    }
}

#[test]
fn a_choice_with_no_options_is_refused() {
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Choice,
            key: "act.mode".to_owned(),
            label: "Mode".to_owned(),
            options: Vec::new(),
            ..PaneControl::default()
        }],
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

#[test]
fn a_dotted_config_key_is_accepted() {
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Choice,
            key: "act.mode".to_owned(),
            label: "Mode".to_owned(),
            options: vec!["learn".to_owned(), "ask".to_owned()],
            ..PaneControl::default()
        }],
        ..base()
    };
    assert!(check_extension(&m).is_ok());
}

#[test]
fn a_config_key_cannot_contain_path_or_quoting_tricks() {
    for key in [
        "act..mode",
        "act.mode.",
        ".mode",
        "act.mo de",
        "act/mode",
        "act.\"mode\"",
    ] {
        let m = ExtensionManifest {
            pane: vec![PaneControl {
                kind: ControlKind::Toggle,
                key: key.to_owned(),
                label: "X".to_owned(),
                ..PaneControl::default()
            }],
            ..base()
        };
        assert!(
            matches!(check_extension(&m), Err(PluginError::BadPane(_))),
            "{key:?} should have been refused"
        );
    }
}

#[test]
fn the_config_file_must_be_a_plain_name() {
    let m = ExtensionManifest {
        config_file: "../../../.ssh/config".to_owned(),
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

#[test]
fn a_command_without_an_id_is_refused() {
    let m = ExtensionManifest {
        commands: vec![PluginCommand::default()],
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

#[test]
fn the_default_kind_is_data_only() {
    // A manifest written before extensions existed must keep meaning
    // exactly what it meant: a language pack.
    assert_eq!(PluginKind::default(), PluginKind::Pack);
    assert_eq!(PluginKind::Pack.as_str(), "pack");
    assert_eq!(PluginKind::Extension.as_str(), "extension");
}
