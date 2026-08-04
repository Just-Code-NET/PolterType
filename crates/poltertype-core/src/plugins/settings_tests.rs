#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

/// A plug-in config in the shape the autopilot actually ships one:
/// mostly prose, because the comments are what tell the user what a
/// switch costs.
const COMMENTED: &str = r#"
# What the autopilot is allowed to do.
#
#   learn  ->  capture only
#   ask    ->  it drafts, you decide
[act]
mode = "learn"          # runtime changes go through the `mode` command

# Ceiling on how long `auto` may be armed for.
max_auto_minutes = 120

[act.model]
# Empty means retrieval only.
name = ""
temperature = 0.4

[capture]
enabled = false
"#;

#[test]
fn reading_finds_values_at_dotted_keys() {
    assert_eq!(
        read_setting(COMMENTED, "act.mode"),
        Some(SettingValue::Text("learn".to_owned()))
    );
    assert_eq!(
        read_setting(COMMENTED, "act.max_auto_minutes"),
        Some(SettingValue::Int(120))
    );
    assert_eq!(
        read_setting(COMMENTED, "capture.enabled"),
        Some(SettingValue::Bool(false))
    );
    assert_eq!(
        read_setting(COMMENTED, "act.model.name"),
        Some(SettingValue::Text(String::new()))
    );
}

#[test]
fn a_key_the_file_does_not_set_reads_as_absent() {
    // Normal, not an error: a config may omit anything it has a
    // default for, and the pane should show the plug-in's default
    // rather than invent one.
    assert_eq!(read_setting(COMMENTED, "act.nothing_here"), None);
    assert_eq!(read_setting(COMMENTED, "no.such.table"), None);
}

#[test]
fn a_table_is_not_a_value() {
    assert_eq!(read_setting(COMMENTED, "act.model"), None);
}

#[test]
fn writing_preserves_every_comment_in_the_file() {
    // The property this module exists for. A parse-and-reserialise
    // round trip would delete all of these the first time somebody
    // touched a toggle.
    let out = write_setting(COMMENTED, "act.mode", &SettingValue::Text("ask".to_owned())).unwrap();
    for comment in [
        "# What the autopilot is allowed to do.",
        "#   learn  ->  capture only",
        "# Ceiling on how long `auto` may be armed for.",
        "# Empty means retrieval only.",
        "# runtime changes go through the `mode` command",
    ] {
        assert!(out.contains(comment), "lost {comment:?} from:\n{out}");
    }
    assert_eq!(
        read_setting(&out, "act.mode"),
        Some(SettingValue::Text("ask".to_owned()))
    );
}

#[test]
fn a_trailing_comment_on_the_edited_line_survives() {
    // Assigning a fresh item over the key takes its decor with it, and
    // the decor is where a trailing comment lives — so the explanation
    // sitting next to a switch would vanish the first time anyone
    // moved that switch.
    let out = write_setting(COMMENTED, "act.mode", &SettingValue::Text("auto".into())).unwrap();
    assert!(
        out.contains("# runtime changes go through the `mode` command"),
        "lost the comment on the edited line:\n{out}"
    );
}

#[test]
fn writing_leaves_every_other_setting_alone() {
    let out = write_setting(COMMENTED, "capture.enabled", &SettingValue::Bool(true)).unwrap();
    assert_eq!(
        read_setting(&out, "act.mode"),
        Some(SettingValue::Text("learn".to_owned()))
    );
    assert_eq!(
        read_setting(&out, "act.model.temperature"),
        Some(SettingValue::Text("0.4".to_owned())),
        "a float should survive untouched"
    );
    assert_eq!(
        read_setting(&out, "capture.enabled"),
        Some(SettingValue::Bool(true))
    );
}

#[test]
fn a_missing_table_is_created_rather_than_refused() {
    // A plug-in's config is allowed to omit a whole section it has
    // defaults for; the pane still has to be able to set it.
    let out = write_setting(
        COMMENTED,
        "act.model.base_url",
        &SettingValue::Text("http://x".into()),
    )
    .unwrap();
    assert_eq!(
        read_setting(&out, "act.model.base_url"),
        Some(SettingValue::Text("http://x".to_owned()))
    );

    let fresh = write_setting("", "brand.new.key", &SettingValue::Int(7)).unwrap();
    assert_eq!(
        read_setting(&fresh, "brand.new.key"),
        Some(SettingValue::Int(7))
    );
}

#[test]
fn writing_through_a_scalar_is_refused_rather_than_clobbering_it() {
    // "act.mode" is a string; "act.mode.deeper" cannot exist without
    // destroying it, and silently destroying a user's setting is worse
    // than refusing the edit.
    let err = write_setting(COMMENTED, "act.mode.deeper", &SettingValue::Int(1)).unwrap_err();
    assert!(matches!(err, PluginError::BadPane(_)), "{err}");
    // And the original is untouched, because nothing was written.
    assert_eq!(
        read_setting(COMMENTED, "act.mode"),
        Some(SettingValue::Text("learn".to_owned()))
    );
}

#[test]
fn overwriting_a_table_with_a_value_is_refused() {
    let err = write_setting(COMMENTED, "act.model", &SettingValue::Int(1)).unwrap_err();
    assert!(matches!(err, PluginError::BadPane(_)), "{err}");
}

#[test]
fn a_config_that_is_not_toml_is_reported_not_overwritten() {
    let err = write_setting("}}} not toml", "a.b", &SettingValue::Bool(true)).unwrap_err();
    assert!(matches!(err, PluginError::BadManifest(_)), "{err}");
}

#[test]
fn values_render_for_a_text_field() {
    assert_eq!(SettingValue::Bool(true).as_display(), "true");
    assert_eq!(SettingValue::Int(42).as_display(), "42");
    assert_eq!(
        SettingValue::Text("mistral-nemo:12b".to_owned()).as_display(),
        "mistral-nemo:12b"
    );
}

#[test]
fn a_round_trip_through_every_kind_keeps_the_document_stable() {
    let mut doc = COMMENTED.to_owned();
    doc = write_setting(&doc, "act.mode", &SettingValue::Text("auto".into())).unwrap();
    doc = write_setting(&doc, "act.max_auto_minutes", &SettingValue::Int(30)).unwrap();
    doc = write_setting(&doc, "capture.enabled", &SettingValue::Bool(true)).unwrap();

    assert_eq!(
        read_setting(&doc, "act.mode"),
        Some(SettingValue::Text("auto".to_owned()))
    );
    assert_eq!(
        read_setting(&doc, "act.max_auto_minutes"),
        Some(SettingValue::Int(30))
    );
    assert_eq!(
        read_setting(&doc, "capture.enabled"),
        Some(SettingValue::Bool(true))
    );
    assert!(doc.contains("# Ceiling on how long"), "{doc}");
}
