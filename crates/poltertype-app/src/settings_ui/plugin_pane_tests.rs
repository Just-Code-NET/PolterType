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
        vec![Slot::control(1)],
        "the report, not the toggle"
    );

    pane.set_output(Slot::control(1), CommandOutput::Loading);
    assert!(
        pane.unasked_commands().is_empty(),
        "asking twice would run the command twice"
    );

    pane.set_output(
        Slot::control(1),
        CommandOutput::Ready("42 episodes".to_owned()),
    );
    assert!(pane.unasked_commands().is_empty());
    assert_eq!(
        pane.output(Slot::control(1)),
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
    pane.set_output(
        Slot::control(0),
        CommandOutput::Failed("it exited 1".to_owned()),
    );
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
        Slot::control(0),
        CommandOutput::Ready(
            "code\tVS Code\t150 episodes\n\nfirefox\n  \nslack\tSlack\tnothing yet\textra\n"
                .to_owned(),
        ),
    );

    let rows = pane.list_rows(Slot::control(0));
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
    assert!(pane.list_rows(Slot::control(0)).is_empty());
    assert_eq!(
        pane.unasked_commands(),
        vec![Slot::control(0)],
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
    pane.set_output(
        Slot::control(0),
        CommandOutput::Ready("code\tVS Code\n".to_owned()),
    );
    assert_eq!(pane.list_rows(Slot::control(0)).len(), 1);

    pane.set_output(
        Slot::control(0),
        CommandOutput::Ready("code\tVS Code\nslack\tSlack\n".to_owned()),
    );
    assert_eq!(
        pane.list_rows(Slot::control(0)).len(),
        2,
        "a refresh redraws the list"
    );

    // And a failure clears them: rows from a previous answer under an
    // error message would be the pane inventing a state.
    pane.set_output(
        Slot::control(0),
        CommandOutput::Failed("it exited 1".to_owned()),
    );
    assert!(pane.list_rows(Slot::control(0)).is_empty());
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
        // A list without a command is refused at manifest load — it has
        // no rows to tick — so a fixture that leaves it empty is testing
        // a pane that cannot exist.
        command: "rooms".to_owned(),
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
    assert_eq!(pane.unasked_commands(), vec![Slot::control(3)]);
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
    assert_eq!(
        pane.unasked_by_command(),
        vec![vec![Slot::control(1), Slot::control(2)]]
    );
    assert_eq!(
        pane.sharing_command(Slot::control(1)),
        vec![Slot::control(1), Slot::control(2)]
    );
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
        pane.flush_edits(Some(&Typing::Control(0)));
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

#[test]
fn select_all_ticks_every_row_on_screen_in_one_write() {
    // Sixty conversations is what this exists for: the user thinks of it
    // as one action, so it is one edit of the file another program is
    // reading.
    let root = scratch("listall");
    let mut pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::List,
            key: "chat.apps.WhatsApp.reply.rooms".to_owned(),
            command: "rows".to_owned(),
            ..PaneControl::default()
        }]),
        &root,
    );
    pane.set_output(
        Slot::control(0),
        CommandOutput::Ready("Чех\tЧех\t\n122 ОБЗ\t122 ОБЗ\tunread\n".to_owned()),
    );

    pane.set_array_all(0, true);
    assert!(pane.in_array(0, "Чех"), "{:?}", pane.status);
    assert!(pane.in_array(0, "122 ОБЗ"));

    pane.set_array_all(0, false);
    assert!(!pane.in_array(0, "Чех"));
    assert!(!pane.in_array(0, "122 ОБЗ"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn select_all_with_nothing_offered_writes_nothing() {
    // No rows means the plug-in has not answered — or has nothing to
    // say. Either way there is nothing to tick, and a write would only
    // create an empty array in somebody else's file.
    let root = scratch("listallempty");
    let mut pane = PluginPane::load(
        extension(vec![control(ControlKind::List, "capture.allow_apps")]),
        &root,
    );
    pane.set_array_all(0, true);
    assert!(pane.status.is_none(), "{:?}", pane.status);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A repeating group whose rows carry a name, a conversation picked from
/// what the plug-in offers, and a button to act on one row.
fn schedule_group() -> PaneControl {
    PaneControl {
        kind: ControlKind::Records,
        key: "schedule.sends".to_owned(),
        label: "The messages".to_owned(),
        id_field: "name".to_owned(),
        actions: vec![poltertype_core::plugins::TrayListAction {
            label: "Send now".to_owned(),
            command: "schedule-run".to_owned(),
        }],
        fields: vec![
            control(ControlKind::Text, "name"),
            PaneControl {
                kind: ControlKind::Suggest,
                key: "room".to_owned(),
                label: "Conversation".to_owned(),
                command: "chat-rooms".to_owned(),
                ..PaneControl::default()
            },
        ],
        ..PaneControl::default()
    }
}

#[test]
fn one_answer_serves_every_card() {
    // Which conversations exist is a question about the chat client, not
    // about the row. Asking it once per card is that client's sidebar
    // read once per scheduled message.
    let root = scratch("records-ask-once");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[[schedule.sends]]\nname = \"morning\"\nroom = \"Чех\"\n\n\
         [[schedule.sends]]\nname = \"standup\"\nroom = \"\"\n",
    )
    .unwrap();

    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    assert_eq!(pane.record_rows(0).len(), 2);
    assert_eq!(
        pane.unasked_commands(),
        vec![Slot {
            control: 0,
            field: Some(1),
            row: None,
        }],
        "one question for the group, not one per card"
    );

    pane.set_output(
        Slot {
            control: 0,
            field: Some(1),
            row: None,
        },
        CommandOutput::Ready("Чех\tЧех\tone-to-one\n122 ОБЗ\t122 ОБЗ\tgroup\n".to_owned()),
    );
    // …and both cards can pick from it.
    for row in 0..2 {
        let offered: Vec<String> = pane
            .suggestions(Slot::field(0, row, 1))
            .into_iter()
            .map(|(value, _)| value)
            .collect();
        assert_eq!(offered, ["Чех".to_owned(), "122 ОБЗ".to_owned()]);
    }
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn picking_a_suggestion_writes_the_id_not_the_label() {
    // What is picked is what is stored. A friendlier label in the list
    // would be a box that saves something other than what it shows —
    // and for a conversation name, one that names nobody.
    let root = scratch("records-pick");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[[schedule.sends]]\nname = \"morning\"\nroom = \"\"\n",
    )
    .unwrap();

    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    pane.set_output(
        Slot {
            control: 0,
            field: Some(1),
            row: None,
        },
        CommandOutput::Ready("Чех\tЧех (3 unread)\tone-to-one\n".to_owned()),
    );
    pane.set_suggestion(Slot::field(0, 0, 1), "Чех");

    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(written.contains("room = \"Чех\""), "{written}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_card_with_no_name_cannot_be_acted_on() {
    // A row action is a command run against a name the plug-in knows.
    let root = scratch("records-id");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[[schedule.sends]]\nname = \"morning\"\n\n[[schedule.sends]]\nname = \"  \"\n",
    )
    .unwrap();

    let pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    assert_eq!(pane.record_id(0, 0).as_deref(), Some("morning"));
    assert_eq!(pane.record_id(0, 1), None);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_message_reaches_the_file_when_it_is_finished_not_while_it_is_typed() {
    // Every keystroke used to settle the previous one, so a message on
    // its way to being written arrived in the plug-in's config one
    // prefix at a time — and the plug-in reads that file to find out
    // what it was asked to send.
    let root = scratch("records-typing");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[[schedule.sends]]\nname = \"morning\"\nroom = \"\"\n",
    )
    .unwrap();

    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    let held = Typing::Record {
        control: 0,
        row: 0,
        field: "room".to_owned(),
    };
    pane.set_record_text(0, 0, "room", "Чех".to_owned());
    pane.flush_edits(Some(&held));
    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(!written.contains("Чех"), "still being typed: {written}");

    // Anything else on the pane settles it.
    pane.flush_edits(None);
    let written = std::fs::read_to_string(&pane.config_path).unwrap();
    assert!(written.contains("room = \"Чех\""), "{written}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn suggestions_are_the_manifests_first_then_the_plugins_without_repeats() {
    let root = scratch("suggest-merge");
    let mut pane = PluginPane::load(
        extension(vec![PaneControl {
            kind: ControlKind::Suggest,
            key: "act.model.base_url".to_owned(),
            label: "Model endpoint".to_owned(),
            command: "endpoints".to_owned(),
            options: vec![poltertype_core::plugins::PaneOption::Value(
                "http://127.0.0.1:11434/v1".to_owned(),
            )],
            ..PaneControl::default()
        }]),
        &root,
    );
    pane.set_output(
        Slot::control(0),
        CommandOutput::Ready(
            "http://127.0.0.1:11434/v1\thttp://127.0.0.1:11434/v1\tanswering\n\
             http://127.0.0.1:11435/v1\thttp://127.0.0.1:11435/v1\tanswering\n"
                .to_owned(),
        ),
    );
    let offered: Vec<String> = pane
        .suggestions(Slot::control(0))
        .into_iter()
        .map(|(value, _)| value)
        .collect();
    assert_eq!(
        offered,
        [
            "http://127.0.0.1:11434/v1".to_owned(),
            "http://127.0.0.1:11435/v1".to_owned()
        ]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn typing_narrows_the_list_and_opens_it() {
    // The list is drawn under the box, so narrowing something you cannot
    // see is not narrowing anything: typing opens it.
    let root = scratch("suggest-narrow");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[[schedule.sends]]\nname = \"morning\"\nroom = \"\"\n",
    )
    .unwrap();

    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    let field = Slot {
        control: 0,
        field: Some(1),
        row: None,
    };
    pane.set_output(
        field,
        CommandOutput::Ready(
            "Чех\tЧех\tone-to-one\n122 ОБЗ\t122 ОБЗ\tgroup\nБронза\tБронза\tgroup\n".to_owned(),
        ),
    );
    let slot = Slot::field(0, 0, 1);
    assert!(!pane.suggest_open(slot), "closed until asked for");
    assert_eq!(pane.suggestions_matching(slot).len(), 3, "all of them");

    pane.set_record_text(0, 0, "room", "ОБ".to_owned());
    assert!(pane.suggest_open(slot), "typing opens it");
    let matching = pane.suggestions_matching(slot);
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].0, "122 ОБЗ");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_button_beside_the_box_opens_the_list_without_typing() {
    let root = scratch("suggest-toggle");
    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    let slot = Slot::field(0, 0, 1);
    assert!(!pane.suggest_open(slot));
    pane.toggle_suggest(slot);
    assert!(pane.suggest_open(slot));
    pane.toggle_suggest(slot);
    assert!(!pane.suggest_open(slot), "and closes it again");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn picking_closes_the_list_and_settles_the_box() {
    // Leaving it up would leave it filtered by a name that is now the
    // answer — a list with one thing in it.
    let root = scratch("suggest-close");
    let dir = root.join("demo-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "[[schedule.sends]]\nname = \"morning\"\nroom = \"\"\n",
    )
    .unwrap();

    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    let slot = Slot::field(0, 0, 1);
    pane.set_record_text(0, 0, "room", "Че".to_owned());
    assert!(pane.suggest_open(slot));

    pane.set_suggestion(slot, "Чех");
    pane.close_suggest();
    assert!(
        !pane.suggest_open(slot),
        "the half-typed filter went with it"
    );
    assert_eq!(pane.record_display(0, 0, "room").as_deref(), Some("Чех"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn only_one_card_may_be_acting_at_a_time() {
    // These steal focus. Two at once would type into each other's
    // window, so the second button is dead while the first runs.
    let root = scratch("action-running");
    let mut pane = PluginPane::load(extension(vec![schedule_group()]), &root);
    assert!(!pane.any_action_running());
    pane.set_action_running(Some((0, 1)));
    assert!(pane.any_action_running());
    assert!(pane.action_running(0, 1));
    assert!(!pane.action_running(0, 0));
    pane.set_action_running(None);
    assert!(!pane.any_action_running());
    std::fs::remove_dir_all(&root).unwrap();
}
