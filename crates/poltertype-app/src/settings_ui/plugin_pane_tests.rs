#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use poltertype_core::plugins::{ExtensionManifest, PluginCommand};

use super::*;

fn control(kind: ControlKind, key: &str) -> PaneControl {
    PaneControl {
        kind,
        key: key.to_owned(),
        label: key.to_owned(),
        ..PaneControl::default()
    }
}

fn extension(pane: Vec<PaneControl>) -> DiscoveredExtension {
    DiscoveredExtension {
        id: "demo-plugin".to_owned(),
        name: "Demo".to_owned(),
        version: "1".to_owned(),
        dir: std::env::temp_dir(),
        exe: PathBuf::from("/bin/true"),
        manifest: ExtensionManifest {
            exe: "true".to_owned(),
            config_file: "config.toml".to_owned(),
            commands: vec![PluginCommand::default()],
            pane,
            ..ExtensionManifest::default()
        },
        development: true,
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ptap-pane-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn values_are_read_from_the_plugins_own_config() {
    let root = scratch("read");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[act]\nmode = \"ask\"\n\n[capture]\nenabled = true\n",
    )
    .unwrap();

    let pane = PluginPane::load(
        extension(vec![
            control(ControlKind::Choice, "act.mode"),
            control(ControlKind::Toggle, "capture.enabled"),
        ]),
        &root,
    );
    assert_eq!(pane.value_of(0), SettingValue::Text("ask".to_owned()));
    assert_eq!(pane.value_of(1), SettingValue::Bool(true));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_key_the_config_omits_shows_a_neutral_default_not_an_invention() {
    // The plug-in's own default applies; we do not know it and must
    // not pretend to.
    let root = scratch("absent");
    let pane = PluginPane::load(
        extension(vec![
            control(ControlKind::Toggle, "capture.enabled"),
            control(ControlKind::Number, "act.max_auto_minutes"),
            control(ControlKind::Text, "act.model.name"),
        ]),
        &root,
    );
    assert!(pane.values.iter().all(Option::is_none));
    assert_eq!(pane.value_of(0), SettingValue::Bool(false));
    assert_eq!(pane.value_of(1), SettingValue::Int(0));
    assert_eq!(pane.value_of(2), SettingValue::Text(String::new()));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn editing_writes_the_file_and_keeps_the_comments() {
    let root = scratch("write");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "# how much authority it has\n[act]\nmode = \"learn\"  # start here\n",
    )
    .unwrap();

    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Choice, "act.mode")]),
        &root,
    );
    pane.set(0, SettingValue::Text("ask".to_owned()));

    let written = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(written.contains("mode = \"ask\""), "{written}");
    assert!(written.contains("# how much authority it has"), "{written}");
    assert!(written.contains("# start here"), "{written}");
    assert_eq!(pane.value_of(0), SettingValue::Text("ask".to_owned()));
    assert!(pane.status.as_deref().unwrap_or_default().contains("Saved"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn editing_creates_the_config_when_the_plugin_has_never_been_configured() {
    let root = scratch("create");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Toggle, "capture.enabled")]),
        &root,
    );
    pane.set(0, SettingValue::Bool(true));

    let written = std::fs::read_to_string(root.join("demo-plugin").join("config.toml")).unwrap();
    assert!(written.contains("enabled = true"), "{written}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_button_control_writes_nothing() {
    // It has no key: it acts rather than stores, and an edit here
    // would be writing to a key the manifest never named.
    let root = scratch("button");
    let mut pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::Button,
            command: "go".to_owned(),
            ..PaneControl::default()
        }]),
        &root,
    );
    pane.set(0, SettingValue::Bool(true));
    assert!(!root.join("demo-plugin").join("config.toml").exists());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_unwritable_key_is_reported_and_the_file_is_left_alone() {
    let root = scratch("refuse");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    let original = "[act]\nmode = \"learn\"\n";
    std::fs::write(dir.join("config.toml"), original).unwrap();

    // "act.mode" is a string, so "act.mode.deeper" cannot be written
    // without destroying it.
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Text, "act.mode.deeper")]),
        &root,
    );
    pane.set(0, SettingValue::Text("x".to_owned()));

    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        original,
        "the plug-in's file must be untouched"
    );
    assert!(pane.status.is_some(), "the refusal must be surfaced");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_plugin_with_no_controls_gets_no_section() {
    let root = scratch("nosection");
    let panes = load_all(vec![extension(Vec::new())], &root);
    assert!(panes.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_config_lives_beside_polterypes_own_not_inside_it() {
    // A plug-in is a separate program; its settings are not a
    // subsection of ours.
    let root = scratch("beside");
    let pane = PluginPane::load(extension(vec![control(ControlKind::Toggle, "a.b")]), &root);
    assert_eq!(
        pane.config_path,
        root.join("demo-plugin").join("config.toml")
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn reports_are_asked_for_once_and_not_on_every_draw() {
    // `view` runs on every frame and a report costs a process, so the
    // pane has to be able to say which ones it has already asked.
    let root = scratch("reports");
    let mut pane = PluginPane::load(
        extension(vec![
            control(ControlKind::Toggle, "a.b"),
            PaneControl {
                kind: ControlKind::Report,
                label: "What it learned".to_owned(),
                command: "report".to_owned(),
                ..PaneControl::default()
            },
        ]),
        &root,
    );
    assert_eq!(
        pane.unasked_commands(),
        vec![1],
        "the report, not the toggle"
    );

    pane.outputs.insert(1, CommandOutput::Loading);
    assert!(
        pane.unasked_commands().is_empty(),
        "asking twice would run the command twice"
    );

    pane.outputs
        .insert(1, CommandOutput::Ready("42 episodes".to_owned()));
    assert!(pane.unasked_commands().is_empty());
    assert_eq!(
        pane.outputs.get(&1),
        Some(&CommandOutput::Ready("42 episodes".to_owned()))
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_pane_with_no_report_asks_for_nothing() {
    let root = scratch("noreports");
    let pane = PluginPane::load(extension(vec![control(ControlKind::Toggle, "a.b")]), &root);
    assert!(pane.unasked_commands().is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_failed_report_is_remembered_rather_than_retried_forever() {
    // A plug-in that cannot answer must not be asked again on every
    // frame; the user asks again with the refresh button.
    let root = scratch("failedreport");
    let mut pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::Report,
            label: "Report".to_owned(),
            command: "report".to_owned(),
            ..PaneControl::default()
        }]),
        &root,
    );
    pane.outputs
        .insert(0, CommandOutput::Failed("it exited 1".to_owned()));
    assert!(pane.unasked_commands().is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn list_rows_are_parsed_leniently() {
    // The plug-in prints something a person could also read. A line
    // with no tabs is a name that is its own label; extra fields are
    // ignored; blank lines are skipped.
    let root = scratch("listrows");
    let mut pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::List,
            key: "capture.allow_apps".to_owned(),
            command: "rows".to_owned(),
            label: "Applications".to_owned(),
            ..PaneControl::default()
        }]),
        &root,
    );
    pane.outputs.insert(
        0,
        CommandOutput::Ready(
            "code\tVS Code\t150 episodes\n\nfirefox\n  \nslack\tSlack\tnothing yet\textra\n"
                .to_owned(),
        ),
    );

    let rows = pane.list_rows(0);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        (rows[0].id.as_str(), rows[0].label.as_str()),
        ("code", "VS Code")
    );
    assert_eq!(rows[0].detail, "150 episodes");
    assert_eq!(
        (rows[1].id.as_str(), rows[1].label.as_str()),
        ("firefox", "firefox"),
        "a bare name is its own label"
    );
    assert_eq!(rows[2].detail, "nothing yet", "the fourth field is ignored");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_list_that_was_never_asked_has_no_rows() {
    let root = scratch("listunasked");
    let pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::List,
            key: "capture.allow_apps".to_owned(),
            command: "rows".to_owned(),
            ..PaneControl::default()
        }]),
        &root,
    );
    assert!(pane.list_rows(0).is_empty());
    assert_eq!(
        pane.unasked_commands(),
        vec![0],
        "and it is still to be asked"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn ticking_a_row_writes_the_plugins_own_config() {
    let root = scratch("listwrite");
    let mut pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::List,
            key: "capture.allow_apps".to_owned(),
            command: "rows".to_owned(),
            ..PaneControl::default()
        }]),
        &root,
    );
    assert!(!pane.in_array(0, "code"));

    pane.set_array_member(0, "code", true);
    assert!(pane.in_array(0, "code"), "{:?}", pane.status);

    pane.set_array_member(0, "code", false);
    assert!(!pane.in_array(0, "code"));
    std::fs::remove_dir_all(&root).unwrap();
}
