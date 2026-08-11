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

    pane.set_output(1, CommandOutput::Loading);
    assert!(
        pane.unasked_commands().is_empty(),
        "asking twice would run the command twice"
    );

    pane.set_output(1, CommandOutput::Ready("42 episodes".to_owned()));
    assert!(pane.unasked_commands().is_empty());
    assert_eq!(
        pane.output(1),
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
    pane.set_output(0, CommandOutput::Failed("it exited 1".to_owned()));
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
    pane.set_output(
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

#[test]
fn a_ticked_box_is_answered_without_touching_the_disk() {
    // `in_array` is called once per row on every frame, so it reads the
    // cache. The regression that matters is the cache going stale: what
    // is on screen has to follow the file, not the last thing this pane
    // happened to know.
    let root = scratch("listcache");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::List, "capture.allow_apps")]),
        &root,
    );
    pane.set_array_member(0, "code", true);
    assert!(pane.in_array(0, "code"), "{:?}", pane.status);

    // The file goes away underneath us. Nothing has told the pane, so
    // the answer is still the one it last read — that is the trade the
    // cache makes, and it is worth stating.
    let config = root.join("demo-plugin").join("config.toml");
    std::fs::write(&config, "[capture]\nallow_apps = []\n").unwrap();
    assert!(pane.in_array(0, "code"), "stale until something re-reads");

    // Reaching a section is such a moment, and the box follows.
    pane.select_section(0);
    assert!(
        !pane.in_array(0, "code"),
        "an edit made elsewhere is picked up on the next step the user takes"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn rows_follow_the_answer_they_were_parsed_from() {
    // The rows are parsed once, when the plug-in answers. A second
    // answer must replace them rather than leave the first one drawn.
    let root = scratch("rowsrefresh");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::List, "capture.allow_apps")]),
        &root,
    );
    pane.set_output(0, CommandOutput::Ready("code\tVS Code\n".to_owned()));
    assert_eq!(pane.list_rows(0).len(), 1);

    pane.set_output(
        0,
        CommandOutput::Ready("code\tVS Code\nslack\tSlack\n".to_owned()),
    );
    assert_eq!(pane.list_rows(0).len(), 2, "a refresh redraws the list");

    // And a failure clears them: rows from a previous answer under an
    // error message would be the pane inventing a state.
    pane.set_output(0, CommandOutput::Failed("it exited 1".to_owned()));
    assert!(pane.list_rows(0).is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

fn section(label: &str) -> PaneControl {
    PaneControl {
        kind: ControlKind::Section,
        label: label.to_owned(),
        ..PaneControl::default()
    }
}

fn listed(key: &str) -> PaneControl {
    PaneControl {
        kind: ControlKind::List,
        key: key.to_owned(),
        label: key.to_owned(),
        command: String::new(),
        ..PaneControl::default()
    }
}

#[test]
fn one_section_is_on_screen_at_a_time() {
    let root = scratch("sections");
    let mut pane = PluginPane::load(
        extension(vec![
            control(ControlKind::Toggle, "capture.enabled"),
            section("Pictures"),
            control(ControlKind::Toggle, "act.vision.enabled"),
            section("Links"),
            control(ControlKind::Toggle, "act.links.enabled"),
        ]),
        &root,
    );

    // Nothing chosen yet: the first section, and whatever was declared
    // above every section.
    assert!(pane.is_visible(0), "a control above every section is shown");
    assert!(pane.is_visible(1), "the first section is the default one");
    assert!(pane.is_visible(2));
    assert!(
        !pane.is_visible(3),
        "another section's heading is in the nav, not the page"
    );
    assert!(!pane.is_visible(4));

    pane.select_section(3);
    assert!(!pane.is_visible(2));
    assert!(pane.is_visible(4));
    assert_eq!(pane.sections(), vec![1, 3]);
}

#[test]
fn a_section_nobody_opened_costs_no_process() {
    // Every command-backed control spawns the plug-in. Asking on behalf
    // of a section that is not on screen is a chat client woken up for
    // a list nobody is looking at.
    let root = scratch("unasked");
    let mut pane = PluginPane::load(
        extension(vec![
            section("Applications"),
            control(ControlKind::Toggle, "capture.enabled"),
            section("Chats"),
            listed("chat.rooms"),
        ]),
        &root,
    );
    assert!(pane.unasked_commands().is_empty());

    pane.select_section(2);
    assert_eq!(pane.unasked_commands(), vec![3]);
}

#[test]
fn two_controls_on_one_command_ask_once() {
    // The rooms an app learns from and the rooms it replies in are the
    // same rooms; asking twice reads the sidebar twice.
    let root = scratch("shared-command");
    let pane = PluginPane::load(
        extension(vec![
            section("Chats"),
            listed("chat.apps.Element.learn.rooms"),
            listed("chat.apps.Element.reply.rooms"),
        ]),
        &root,
    );
    assert_eq!(pane.unasked_by_command(), vec![vec![1, 2]]);
    assert_eq!(pane.sharing_command(1), vec![1, 2]);
}

#[test]
fn a_plugin_with_no_sections_shows_everything() {
    let root = scratch("no-sections");
    let pane = PluginPane::load(
        extension(vec![
            control(ControlKind::Toggle, "a.b"),
            control(ControlKind::Toggle, "c.d"),
        ]),
        &root,
    );
    assert!(pane.is_visible(0) && pane.is_visible(1));
    assert!(pane.sections().is_empty());
}

#[test]
fn typing_reaches_the_file_only_once_it_settles() {
    // The prefixes of "0.85" include "0", and a threshold that is 0 for
    // the length of a keystroke is a gate the user never opened. So
    // nothing is written until the user does something else.
    let root = scratch("decimal");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Decimal, "act.min_confidence")]),
        &root,
    );

    for prefix in ["0", "0.", "0.8", "0.85"] {
        pane.set_text(0, prefix.to_owned());
        pane.flush_edits(Some(0));
        assert!(
            !pane.config_path.exists(),
            "still typing: nothing should have been written yet"
        );
    }

    pane.flush_edits(None);
    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(written.contains("min_confidence = 0.85"), "{written}");
}

#[test]
fn a_half_typed_decimal_stays_in_the_box_and_out_of_the_file() {
    let root = scratch("decimal-half");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Decimal, "act.min_confidence")]),
        &root,
    );

    pane.set_text(0, "-".to_owned());
    pane.flush_edits(None);
    assert_eq!(
        pane.display_of(0).as_deref(),
        Some("-"),
        "what cannot be written is still what the user sees"
    );
    assert!(!pane.config_path.exists(), "half a number is not a number");
}

#[test]
fn a_whole_number_typed_into_a_decimal_is_written_as_a_decimal() {
    let root = scratch("decimal-round");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Decimal, "act.humanize.type_cps")]),
        &root,
    );
    pane.set_text(0, "6".to_owned());
    pane.flush_edits(None);
    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(
        written.contains("type_cps = 6.0"),
        "a plug-in expecting a float cannot read an integer: {written}"
    );
}

#[test]
fn a_typed_list_round_trips_through_the_file() {
    let root = scratch("strings");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Strings, "act.links.allow_hosts")]),
        &root,
    );

    pane.set_text(0, "github.com, docs.rs".to_owned());
    pane.flush_edits(None);
    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(
        written.contains("allow_hosts = [\"github.com\", \"docs.rs\"]"),
        "{written}"
    );

    // Re-read the way opening the window does.
    let reloaded = PluginPane::load(
        extension(vec![control(ControlKind::Strings, "act.links.allow_hosts")]),
        &root,
    );
    assert_eq!(
        reloaded.display_of(0).as_deref(),
        Some("github.com, docs.rs")
    );
}

#[test]
fn a_trailing_comma_does_not_put_an_empty_name_in_the_list() {
    // These lists are matched as substrings; an empty member would
    // match every conversation there is.
    let root = scratch("strings-comma");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::Strings, "act.awayreply.rooms")]),
        &root,
    );
    pane.set_text(0, "Піккатцо, ".to_owned());
    pane.flush_edits(None);
    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(written.contains("rooms = [\"Піккатцо\"]"), "{written}");
}
