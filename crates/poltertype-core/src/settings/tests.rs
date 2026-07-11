use super::*;

#[test]
fn defaults_serialise_and_round_trip() {
    let s = Settings::default();
    let serialized = toml::to_string_pretty(&s).expect("serialize");
    let back: Settings = toml::from_str(&serialized).expect("parse");
    assert_eq!(s, back);
}

#[test]
fn missing_keys_use_defaults() {
    // Minimal valid TOML — every section uses its `Default::default()`.
    let s: Settings = toml::from_str("schema_version = 1").expect("parse");
    assert_eq!(s.engine.min_word_length, 3);
    assert_eq!(s.general.log_level, "info");
    assert!(!s.ai.enabled);
    assert!(s.engine.suppress_in_identifiers);
    assert!(s.engine.suppress_for_all_caps);
}

/// Forward-compat regression: a config that's missing a struct
/// field added after the user wrote the file must still parse —
/// that's what `#[serde(default)]` on every settings struct buys
/// us.
#[test]
fn old_config_missing_new_field_still_parses() {
    let raw = "schema_version = 1\n\n[engine]\nmin_word_length = 4\nconfidence_threshold = 0.7\n";
    let s: Settings = toml::from_str(raw).expect("parse");
    assert_eq!(s.engine.min_word_length, 4);
    // `suppress_in_identifiers` / `suppress_for_all_caps` were
    // missing from the user's file but the defaults kicked in.
    assert!(s.engine.suppress_in_identifiers);
    assert!(s.engine.suppress_for_all_caps);
}

/// User commands sit in their own `[[commands]]` table. A full
/// config block including one must round-trip through the live
/// `Settings` struct — the regression we care about is that
/// `CommandsSettings` is wired in correctly (no `serde(skip)`,
/// no `default` collision dropping the user data on save).
#[test]
fn commands_section_round_trips_inside_full_settings() {
    let raw = r#"
schema_version = 1

[[commands]]
id      = "anrl"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }
"#;
    let parsed: Settings = toml::from_str(raw).expect("parse with commands");
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(parsed.commands[0].id, "anrl");
    assert_eq!(parsed.commands[0].trigger, "anrl");

    // And the round-trip back to TOML must preserve the entry —
    // a `Default` collision or stray `serde(skip)` would silently
    // drop it on first save, which is the worst kind of bug.
    let serialised = toml::to_string_pretty(&parsed).expect("serialise");
    let back: Settings = toml::from_str(&serialised).expect("parse round-trip");
    assert_eq!(back.commands.len(), 1);
    assert_eq!(back.commands[0].id, "anrl");
    assert_eq!(back.commands[0].trigger, "anrl");
}

/// Legacy configs from beta.4 and earlier had no `[[commands]]`
/// section. They must still parse — the user shouldn't have to
/// edit their config to keep the app starting.
#[test]
fn legacy_config_without_commands_still_parses() {
    let raw = r#"
schema_version = 1

[hotkeys]
pause_toggle = "Ctrl+Shift+Space"
manual_switch_last = "Ctrl+Shift+Backspace"
"#;
    let parsed: Settings = toml::from_str(raw).expect("parse legacy");
    assert!(parsed.commands.is_empty());
    assert_eq!(parsed.hotkeys.pause_toggle, "Ctrl+Shift+Space");
}

#[test]
fn default_disabled_apps_covers_common_editors() {
    let s = Settings::default();
    let lower: Vec<String> = s
        .exceptions
        .disabled_apps
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    for must in ["code.exe", "cursor.exe", "windowsterminal.exe", "alacritty"] {
        assert!(
            lower.iter().any(|s| s == must),
            "expected `{must}` in default disabled_apps"
        );
    }
}
