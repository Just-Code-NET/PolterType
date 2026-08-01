use poltertype_types::LayoutId;
use serde::Deserialize;

use super::*;

/// The TOML-tag form must round-trip every variant — that's the
/// shape users will type into `config.toml`, so any rename or
/// field reorder breaks every existing config.
#[test]
fn each_action_variant_round_trips_through_toml() {
    let cases = [
        (
            "type_text",
            CommandAction::TypeText {
                text: "hello".into(),
            },
        ),
        (
            "switch_layout",
            CommandAction::SwitchLayout {
                layout: LayoutId::new("en-US"),
            },
        ),
        (
            "open_path",
            CommandAction::OpenPath {
                path: "C:/work/notes.md".into(),
            },
        ),
    ];
    for (tag, action) in cases {
        let toml_str = toml::to_string(&action).expect("serialise");
        assert!(
            toml_str.contains(&format!("type = \"{tag}\"")),
            "expected `type = \"{tag}\"` in serialised form, got: {toml_str}"
        );
        let back: CommandAction = toml::from_str(&toml_str).expect("parse");
        assert_eq!(action, back);
    }
}

/// A complete config snippet a user might write — array-of-tables
/// `[[commands]]` parses end-to-end through the wrapper used in
/// `Settings`. Triggers are TYPED text, not hotkey combos.
#[test]
fn parses_complete_user_command_block() {
    // We mirror the wrapper that `Settings.commands` will use:
    // a top-level `commands` field of type `Vec<UserCommand>`.
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default)]
        commands: Vec<UserCommand>,
    }
    let raw = r#"
[[commands]]
id      = "anrl"
name    = "Anatomical reference list"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }
apps    = ["thunderbird.exe", "OUTLOOK.EXE"]

[[commands]]
id      = "to-english"
trigger = "((en))"
action  = { type = "switch_layout", layout = "en-US" }
"#;
    let w: Wrap = toml::from_str(raw).expect("parse");
    assert_eq!(w.commands.len(), 2);

    let anrl = &w.commands[0];
    assert_eq!(anrl.id, "anrl");
    assert_eq!(anrl.name, "Anatomical reference list");
    assert_eq!(anrl.trigger, "anrl");
    assert!(matches!(
        &anrl.action,
        CommandAction::TypeText { text } if text == "Anatomical Reference List"
    ));
    assert_eq!(anrl.apps.len(), 2);

    let lang = &w.commands[1];
    assert_eq!(lang.id, "to-english");
    // `name` defaults to empty — UI falls back to `id` for display.
    assert!(lang.name.is_empty());
    assert!(lang.apps.is_empty());
    assert!(matches!(
        &lang.action,
        CommandAction::SwitchLayout { layout } if layout.as_str() == "en-US"
    ));
}

/// Reserved built-in ids must be exposed as constants — the UI
/// validation layer consults them to refuse or warn when a
/// user tries to shadow `pause-toggle` / `switch-last`.
#[test]
fn builtin_ids_are_distinct_and_kebab_case() {
    assert_ne!(BUILTIN_PAUSE_TOGGLE_ID, BUILTIN_SWITCH_LAST_ID);
    for id in [BUILTIN_PAUSE_TOGGLE_ID, BUILTIN_SWITCH_LAST_ID] {
        assert!(id.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
    }
}

fn cmd(id: &str, trigger: &str, apps: &[&str]) -> UserCommand {
    UserCommand {
        id: id.into(),
        name: String::new(),
        trigger: trigger.into(),
        action: CommandAction::TypeText { text: "x".into() },
        apps: apps.iter().map(|s| (*s).into()).collect(),
    }
}

/// Trigger matching is exact, case-sensitive, and consults the
/// optional `apps` filter. These three properties together are
/// what users will lean on when designing triggers — getting
/// any one of them wrong breaks expectations silently.
#[test]
fn find_matching_command_basic_match() {
    let list = vec![cmd("a", "anrl", &[]), cmd("b", "((en))", &[])];

    // Exact match.
    let m = find_matching_command(&list, "anrl", None, &WordHistory::default()).expect("matches");
    assert_eq!(m.id, "a");
    // Different trigger.
    let m2 =
        find_matching_command(&list, "((en))", None, &WordHistory::default()).expect("matches");
    assert_eq!(m2.id, "b");
    // No match — case-sensitive, "ANRL" != "anrl".
    assert!(find_matching_command(&list, "ANRL", None, &WordHistory::default()).is_none());
    // No match — completely unrelated word.
    assert!(find_matching_command(&list, "hello", None, &WordHistory::default()).is_none());
    // No match — empty word.
    assert!(find_matching_command(&list, "", None, &WordHistory::default()).is_none());
}

/// `apps` filter: empty list = match anywhere; non-empty = the
/// focused app's basename must match (case-insensitive).
#[test]
fn find_matching_command_app_filter() {
    let list = vec![cmd("a", "anrl", &["Code.exe", "idea64.exe"])];

    // No app reported → fail closed (filter set, can't verify).
    assert!(find_matching_command(&list, "anrl", None, &WordHistory::default()).is_none());
    // Wrong app → no match.
    assert!(
        find_matching_command(&list, "anrl", Some("chrome.exe"), &WordHistory::default()).is_none()
    );
    // Right app, exact case → match.
    assert!(
        find_matching_command(&list, "anrl", Some("Code.exe"), &WordHistory::default()).is_some()
    );
    // Right app, wrong case → still match (case-insensitive
    // basename comparison, mirrors `disabled_apps` rules).
    assert!(
        find_matching_command(&list, "anrl", Some("CODE.EXE"), &WordHistory::default()).is_some()
    );
}

/// First match wins when two commands share a trigger. That's
/// a config error in practice — but we resolve it
/// deterministically rather than crashing the engine.
#[test]
fn find_matching_command_first_match_wins() {
    let list = vec![cmd("a", "dup", &[]), cmd("b", "dup", &[])];
    let m = find_matching_command(&list, "dup", None, &WordHistory::default()).expect("matches");
    assert_eq!(m.id, "a");
}
