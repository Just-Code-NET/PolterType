use super::*;

use crate::i18n::Catalog;
use crate::plugins::{ControlKind, TrayListAction};

fn manifest() -> ExtensionManifest {
    ExtensionManifest {
        summary: "Answers your chats".to_owned(),
        pane: vec![
            PaneControl {
                kind: ControlKind::Section,
                label: "How it acts".to_owned(),
                help: "What it does when a message arrives.".to_owned(),
                ..Default::default()
            },
            PaneControl {
                kind: ControlKind::Choice,
                key: "act.mode".to_owned(),
                label: "Mode".to_owned(),
                options: vec![
                    PaneOption::Value("off".to_owned()),
                    PaneOption::Described {
                        value: "auto".to_owned(),
                        label: "Automatic".to_owned(),
                        detail: "Sends without asking.".to_owned(),
                        link: String::new(),
                    },
                ],
                ..Default::default()
            },
            PaneControl {
                kind: ControlKind::Records,
                key: "schedule.sends".to_owned(),
                label: "Scheduled messages".to_owned(),
                add_label: "Add a message".to_owned(),
                fields: vec![PaneControl {
                    kind: ControlKind::Text,
                    key: "room".to_owned(),
                    label: "Room".to_owned(),
                    ..Default::default()
                }],
                actions: vec![TrayListAction {
                    label: "Send now".to_owned(),
                    command: "send-now".to_owned(),
                }],
                id_field: "room".to_owned(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

fn catalog(body: &str) -> Catalog {
    Catalog::parse("uk", body, "<test>")
}

#[test]
fn keys_are_derived_from_the_manifests_own_structure() {
    let keys: Vec<String> = strings(&manifest()).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        vec![
            "summary",
            "pane.how_it_acts.label",
            "pane.how_it_acts.help",
            "pane.act.mode.label",
            "pane.act.mode.option.off",
            "pane.act.mode.option.auto",
            "pane.act.mode.option.auto.detail",
            "pane.schedule.sends.label",
            "pane.schedule.sends.add",
            "pane.schedule.sends.action.send-now",
            "pane.schedule.sends.field.room.label",
        ]
    );
}

/// The template is what a translator edits, so it has to arrive with
/// the English in it rather than blanks to fill from the manifest.
#[test]
fn the_template_carries_the_english() {
    let pairs = strings(&manifest());
    assert!(pairs.contains(&("summary".to_owned(), "Answers your chats".to_owned())));
    assert!(pairs.contains(&("pane.act.mode.option.off".to_owned(), "off".to_owned())));
}

#[test]
fn a_translation_replaces_the_label_and_nothing_else() {
    let mut m = manifest();
    localise_with(
        &mut m,
        "acme",
        &catalog(
            r#"
"plugin.acme.summary" = "Відповідає у чатах"
"plugin.acme.pane.act.mode.label" = "Режим"
"plugin.acme.pane.schedule.sends.field.room.label" = "Кімната"
"#,
        ),
    );
    assert_eq!(m.summary, "Відповідає у чатах");
    assert_eq!(m.pane[1].label, "Режим");
    assert_eq!(m.pane[1].key, "act.mode", "the config key is untouched");
    assert_eq!(m.pane[2].fields[0].label, "Кімната");
    assert_eq!(m.pane[2].fields[0].key, "room");
}

/// A bare option is its own label, so translating it has to keep the
/// value behind: that string is what lands in the plug-in's config.
#[test]
fn a_translated_option_still_writes_the_declared_value() {
    let mut m = manifest();
    localise_with(
        &mut m,
        "acme",
        &catalog(
            r#"
"plugin.acme.pane.act.mode.option.off" = "Вимкнено"
"plugin.acme.pane.act.mode.option.auto" = "Автоматично"
"plugin.acme.pane.act.mode.option.auto.detail" = "Надсилає без запиту."
"#,
        ),
    );
    let options = &m.pane[1].options;
    assert_eq!(options[0].value(), "off");
    assert_eq!(options[0].label(), "Вимкнено");
    assert_eq!(options[1].value(), "auto");
    assert_eq!(options[1].label(), "Автоматично");
    assert_eq!(options[1].detail(), "Надсилає без запиту.");
    // A translated word is not a reason to redraw a drop-down as cards.
    assert!(!options[0].is_described());
}

#[test]
fn an_untranslated_key_stays_in_the_language_the_manifest_declared() {
    let mut m = manifest();
    localise_with(
        &mut m,
        "acme",
        &catalog("\"plugin.acme.summary\" = \"Х\"\n"),
    );
    assert_eq!(m.pane[1].label, "Mode");
    assert_eq!(m.pane[2].add_label, "Add a message");
}

/// Another plug-in's catalog, and PolterType's own keys, are simply not
/// where this plug-in looks — the confinement in `Catalog::overlay` is
/// the belt, this is the braces.
#[test]
fn a_plugin_reads_only_its_own_namespace() {
    let mut m = manifest();
    localise_with(
        &mut m,
        "acme",
        &catalog(
            r#"
"summary" = "not namespaced"
"plugin.other.summary" = "someone else's"
"footer.save" = "PolterType's own"
"#,
        ),
    );
    assert_eq!(m.summary, "Answers your chats");
}

#[test]
fn a_label_with_no_key_of_its_own_is_slugged() {
    assert_eq!(slug("How it acts"), "how_it_acts");
    assert_eq!(slug("  Ready?  "), "ready");
    assert_eq!(slug("A/B — testing"), "a_b_testing");
    assert_eq!(slug("!!!"), "");
    assert!(slug(&"word ".repeat(20)).len() <= SLUG_MAX);
}
