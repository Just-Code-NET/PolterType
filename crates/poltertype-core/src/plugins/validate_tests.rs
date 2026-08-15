#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::plugins::enums::PluginKind;
use crate::plugins::types::{
    ExtensionManifest, PaneControl, PaneOption, PluginCommand, TrayItem, TrayListAction,
};

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
                options: vec![PaneOption::Value("a".to_owned())],
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
            options: vec![
                PaneOption::Value("learn".to_owned()),
                PaneOption::Value("ask".to_owned()),
            ],
            ..PaneControl::default()
        }],
        ..base()
    };
    assert!(check_extension(&m).is_ok());
}

/// A choice control carrying whatever options are handed to it.
fn choice_with(options: Vec<PaneOption>) -> ExtensionManifest {
    ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Choice,
            key: "act.model.name".to_owned(),
            label: "Model".to_owned(),
            options,
            ..PaneControl::default()
        }],
        ..base()
    }
}

#[test]
fn an_option_may_describe_itself_and_the_plain_form_still_works() {
    let m = choice_with(vec![
        PaneOption::Value("off".to_owned()),
        PaneOption::Described {
            value: "qwen3:8b".to_owned(),
            label: "Qwen3 8B".to_owned(),
            detail: "Fits an 8 GB card whole.".to_owned(),
            link: "https://ollama.com/library/qwen3".to_owned(),
        },
    ]);
    assert!(check_extension(&m).is_ok());

    let opts = &m.pane[0].options;
    // A bare value is its own label and says nothing more.
    assert_eq!(opts[0].value(), "off");
    assert_eq!(opts[0].label(), "off");
    assert!(!opts[0].is_described());
    // A described one keeps the two apart: what is written into the
    // config is never the friendly name.
    assert_eq!(opts[1].value(), "qwen3:8b");
    assert_eq!(opts[1].label(), "Qwen3 8B");
    assert!(opts[1].is_described());
}

#[test]
fn both_forms_of_option_parse_from_one_array() {
    // The point of the untagged form: a plug-in describes the options
    // that need it and leaves the rest as strings, in one list, with no
    // parallel array to keep in step.
    let toml = r#"
        id = "x"
        name = "X"
        version = "1"
        kind = "extension"
        [extension]
        exe = "x"
        [[extension.pane]]
        kind = "choice"
        key = "act.model.name"
        label = "Model"
        options = [
          "off",
          { value = "qwen3:8b", detail = "Fits.", link = "https://ollama.com/library/qwen3" },
        ]
    "#;
    let m: ExtensionManifest = toml::from_str::<toml::Value>(toml)
        .unwrap()
        .get("extension")
        .unwrap()
        .clone()
        .try_into()
        .unwrap();
    let opts = &m.pane[0].options;
    assert_eq!(opts.len(), 2);
    assert_eq!(opts[0].value(), "off");
    assert_eq!(opts[1].value(), "qwen3:8b");
    assert_eq!(opts[1].detail(), "Fits.");
}

#[test]
fn a_link_that_is_not_https_is_refused_rather_than_drawn() {
    // A link in a manifest is a third party naming a place PolterType
    // will send somebody. A `file://` or a `javascript:` beside an
    // innocuous label is the whole reason this pane draws everything
    // itself, so the check belongs at load, not at click.
    for bad in [
        "http://example.com",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ollama.com/library/qwen3",
    ] {
        let m = choice_with(vec![PaneOption::Described {
            value: "v".to_owned(),
            label: String::new(),
            detail: String::new(),
            link: bad.to_owned(),
        }]);
        assert!(
            matches!(check_extension(&m), Err(PluginError::BadPane(_))),
            "{bad:?} should have been refused"
        );
    }
}

#[test]
fn an_option_with_no_value_is_refused() {
    // It would draw a radio button that writes an empty setting.
    let m = choice_with(vec![PaneOption::Value("  ".to_owned())]);
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
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

#[test]
fn a_report_must_name_a_command_that_exists() {
    // A report pointing at nothing would render an empty block for
    // ever, which reads as "this plug-in has nothing to say" rather
    // than as the manifest error it is.
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Report,
            label: "What it learned".to_owned(),
            command: "no-such-command".to_owned(),
            ..PaneControl::default()
        }],
        ..base()
    };
    match check_extension(&m) {
        Err(PluginError::BadPane(why)) => {
            assert!(why.contains("report"), "{why}");
            assert!(why.contains("no-such-command"), "{why}");
        }
        other => panic!("expected a BadPane error, got {other:?}"),
    }
}

#[test]
fn a_report_naming_a_declared_command_is_accepted() {
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Report,
            label: "What it learned".to_owned(),
            command: "run".to_owned(),
            ..PaneControl::default()
        }],
        ..base()
    };
    assert!(check_extension(&m).is_ok());
}

#[test]
fn a_report_needs_no_key_because_it_writes_nothing() {
    // Every other control binds to a config key. This one reads, so
    // requiring a key would be asking a manifest author to invent one.
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Report,
            label: "Report".to_owned(),
            command: "run".to_owned(),
            key: String::new(),
            ..PaneControl::default()
        }],
        ..base()
    };
    assert!(check_extension(&m).is_ok());
}

#[test]
fn a_control_from_a_newer_polterype_does_not_take_the_manifest_with_it() {
    // The compatibility rule that makes adding kinds safe: a plug-in
    // written for a newer app must still load here, minus the control
    // nobody can draw. Refusing the file would make the whole plug-in
    // vanish from the pane because of one unfamiliar word.
    let manifest: ExtensionManifest = toml::from_str(
        r#"
        exe = "some-plugin"
        [[commands]]
        id = "run"
        args = ["run"]
        [[pane]]
        kind = "hologram"
        label = "From the future"
        "#,
    )
    .expect("an unknown control kind must not fail the parse");
    assert_eq!(manifest.pane[0].kind, ControlKind::Unknown);
    assert!(check_extension(&manifest).is_ok());
}

#[test]
fn a_section_needs_something_to_say() {
    let m = ExtensionManifest {
        exe: "demo".to_owned(),
        pane: vec![PaneControl {
            kind: ControlKind::Section,
            ..PaneControl::default()
        }],
        ..ExtensionManifest::default()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

#[test]
fn a_typed_list_and_a_decimal_both_need_a_key() {
    for kind in [ControlKind::Strings, ControlKind::Decimal] {
        let m = ExtensionManifest {
            exe: "demo".to_owned(),
            pane: vec![PaneControl {
                kind,
                label: "no key".to_owned(),
                ..PaneControl::default()
            }],
            ..ExtensionManifest::default()
        };
        assert!(
            matches!(check_extension(&m), Err(PluginError::BadPane(_))),
            "{kind:?} without a key must be refused"
        );
    }
}

fn suggest(key: &str) -> PaneControl {
    PaneControl {
        kind: ControlKind::Suggest,
        key: key.to_owned(),
        label: key.to_owned(),
        command: "run".to_owned(),
        ..PaneControl::default()
    }
}

#[test]
fn a_box_that_suggests_must_have_something_to_suggest() {
    // Neither a list nor a command is a plain text box wearing a
    // drop-down arrow that never opens.
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            command: String::new(),
            ..suggest("schedule.sends")
        }],
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));

    // Either one on its own is enough.
    let from_command = ExtensionManifest {
        pane: vec![suggest("act.model.base_url")],
        ..base()
    };
    assert!(check_extension(&from_command).is_ok());

    let from_options = ExtensionManifest {
        pane: vec![PaneControl {
            command: String::new(),
            options: vec![PaneOption::Value("weekdays 09:00".to_owned())],
            ..suggest("act.model.base_url")
        }],
        ..base()
    };
    assert!(check_extension(&from_options).is_ok());
}

#[test]
fn a_box_that_suggests_may_not_name_a_command_nobody_declared() {
    let m = ExtensionManifest {
        pane: vec![PaneControl {
            command: "rooms".to_owned(),
            ..suggest("schedule.room")
        }],
        ..base()
    };
    assert!(matches!(check_extension(&m), Err(PluginError::BadPane(_))));
}

fn group_with_actions(id_field: &str, command: &str) -> ExtensionManifest {
    ExtensionManifest {
        pane: vec![PaneControl {
            kind: ControlKind::Records,
            key: "schedule.sends".to_owned(),
            label: "The messages".to_owned(),
            id_field: id_field.to_owned(),
            actions: vec![TrayListAction {
                label: "Send now".to_owned(),
                command: command.to_owned(),
            }],
            fields: vec![PaneControl {
                kind: ControlKind::Text,
                key: "name".to_owned(),
                label: "Name".to_owned(),
                ..PaneControl::default()
            }],
            ..PaneControl::default()
        }],
        ..base()
    }
}

#[test]
fn a_row_action_must_know_what_the_row_is_called() {
    // `{id}` with nothing to fill it in would run "send the message
    // called X" against the literal string.
    assert!(check_extension(&group_with_actions("name", "run")).is_ok());
    assert!(matches!(
        check_extension(&group_with_actions("", "run")),
        Err(PluginError::BadPane(_))
    ));
    assert!(matches!(
        check_extension(&group_with_actions("title", "run")),
        Err(PluginError::BadPane(_))
    ));
    assert!(matches!(
        check_extension(&group_with_actions("name", "send")),
        Err(PluginError::BadPane(_))
    ));
}
