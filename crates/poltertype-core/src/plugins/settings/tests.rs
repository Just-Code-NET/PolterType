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
        Some(SettingValue::Float(0.4)),
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

// ── arrays: the set a control adds itself to ────────────────────────

#[test]
fn ticking_a_row_adds_it_and_unticking_takes_it_out() {
    let text = "[capture]\nallow_apps = [\"code\"]\n";
    let with = set_array_member(text, "capture.allow_apps", "firefox", true).unwrap();
    assert_eq!(
        read_string_array(&with, "capture.allow_apps"),
        ["code", "firefox"]
    );

    let without = set_array_member(&with, "capture.allow_apps", "code", false).unwrap();
    assert_eq!(
        read_string_array(&without, "capture.allow_apps"),
        ["firefox"]
    );
}

#[test]
fn the_comments_explaining_the_list_survive_an_edit() {
    // The file this was written for uses the allow-list's comments to
    // explain why certain applications are deliberately absent. An
    // edit that reformatted the array would delete the reasoning.
    let text = "\
# what we learn from
[capture]
allow_apps = [
    \"code\",  # the editor
    # kitty is DELIBERATELY absent: shells hold credentials
]
";
    let out = set_array_member(text, "capture.allow_apps", "firefox", true).unwrap();
    assert!(out.contains("DELIBERATELY absent"), "{out}");
    assert!(out.contains("# the editor"), "{out}");
    assert!(out.contains("firefox"), "{out}");
}

#[test]
fn a_missing_array_is_created_rather_than_refused() {
    let out = set_array_member("", "capture.allow_apps", "code", true).unwrap();
    assert_eq!(read_string_array(&out, "capture.allow_apps"), ["code"]);
}

#[test]
fn a_missing_array_reads_as_empty_not_as_an_error() {
    assert!(read_string_array("", "capture.allow_apps").is_empty());
    assert!(read_string_array("[capture]\n", "capture.allow_apps").is_empty());
    // A key that holds something else is not an array, and saying so
    // by returning nothing is better than guessing at its contents.
    assert!(read_string_array("[capture]\nallow_apps = 3\n", "capture.allow_apps").is_empty());
}

#[test]
fn adding_twice_or_removing_what_is_absent_changes_nothing() {
    let text = "[capture]\nallow_apps = [\"code\"]\n";
    let same = set_array_member(text, "capture.allow_apps", "code", true).unwrap();
    assert_eq!(read_string_array(&same, "capture.allow_apps"), ["code"]);
    let still = set_array_member(&same, "capture.allow_apps", "firefox", false).unwrap();
    assert_eq!(read_string_array(&still, "capture.allow_apps"), ["code"]);
}

#[test]
fn a_key_that_is_not_an_array_is_refused_rather_than_overwritten() {
    let text = "[capture]\nallow_apps = \"code\"\n";
    assert!(set_array_member(text, "capture.allow_apps", "firefox", true).is_err());
}

#[test]
fn a_float_stays_a_float_through_a_round_trip() {
    // The one that matters: a decimal key handed back as an integer is
    // a config the plug-in refuses to parse at all.
    let value = read_setting(COMMENTED, "act.model.temperature").unwrap();
    assert_eq!(value, SettingValue::Float(0.4));

    let updated = write_setting(COMMENTED, "act.model.temperature", &value).unwrap();
    assert!(updated.contains("temperature = 0.4"), "{updated}");

    let round = write_setting(
        COMMENTED,
        "act.model.temperature",
        &SettingValue::Float(1.0),
    )
    .unwrap();
    assert!(round.contains("temperature = 1.0"), "{round}");
}

#[test]
fn a_round_float_still_shows_its_point() {
    assert_eq!(SettingValue::Float(25.0).as_display(), "25.0");
    assert_eq!(SettingValue::Float(0.35).as_display(), "0.35");
}

#[test]
fn writing_a_list_replaces_the_members_and_keeps_the_prose() {
    let text =
        "# which hosts may be opened\n[act.links]\nallow_hosts = [\"a.example\"]  # for now\n";
    let updated = write_string_array(
        text,
        "act.links.allow_hosts",
        &["github.com".to_owned(), "docs.rs".to_owned()],
    )
    .unwrap();

    assert_eq!(
        read_string_array(&updated, "act.links.allow_hosts"),
        vec!["github.com".to_owned(), "docs.rs".to_owned()]
    );
    assert!(updated.contains("# which hosts may be opened"), "{updated}");
    assert!(updated.contains("# for now"), "{updated}");
}

#[test]
fn writing_a_list_into_a_file_that_has_no_table_yet() {
    let updated = write_string_array(
        "",
        "chat.apps.Element.reply.rooms",
        &["Піккатцо".to_owned()],
    )
    .unwrap();
    assert_eq!(
        read_string_array(&updated, "chat.apps.Element.reply.rooms"),
        vec!["Піккатцо".to_owned()]
    );
}

#[test]
fn emptying_a_list_writes_an_empty_array_rather_than_removing_it() {
    // "No hosts" and "the key is absent, so the plug-in's default
    // applies" are different permissions, and only one of them is what
    // the user just asked for.
    let updated = write_string_array(
        "[act.links]\nallow_hosts = [\"a.example\"]\n",
        "act.links.allow_hosts",
        &[],
    )
    .unwrap();
    assert!(updated.contains("allow_hosts = []"), "{updated}");
}

#[test]
fn a_list_key_that_names_a_table_is_refused() {
    let err = write_string_array("[act.links]\n", "act.links", &["x".to_owned()]).unwrap_err();
    assert!(matches!(err, PluginError::BadPane(_)), "{err}");
}

#[test]
fn selecting_all_writes_the_whole_set_once_and_keeps_the_rest() {
    // "Select all" is one action to the person taking it, so it is one
    // edit to the file — and it must not disturb what was already there,
    // including a name the plug-in did not offer this time.
    let text = "[chat.apps.WhatsApp.reply]\nrooms = [\"Чех\", \"a chat that is offline today\"]\n";
    let out = set_array_members(
        text,
        "chat.apps.WhatsApp.reply.rooms",
        &["Чех", "122 ОБЗ", "Бронза"],
        true,
    )
    .unwrap();
    assert_eq!(
        read_string_array(&out, "chat.apps.WhatsApp.reply.rooms"),
        ["Чех", "a chat that is offline today", "122 ОБЗ", "Бронза"],
        "already-ticked rows are not duplicated, and unoffered ones stay"
    );
}

#[test]
fn clearing_removes_only_what_was_offered() {
    // The user is acting on the list they can see. A conversation stored
    // for a client that is not running is not on screen, and taking it
    // out would be the one surprise this control must not spring.
    let text = "[chat.apps.WhatsApp.reply]\nrooms = [\"Чех\", \"122 ОБЗ\", \"offline chat\"]\n";
    let out = set_array_members(
        text,
        "chat.apps.WhatsApp.reply.rooms",
        &["Чех", "122 ОБЗ"],
        false,
    )
    .unwrap();
    assert_eq!(
        read_string_array(&out, "chat.apps.WhatsApp.reply.rooms"),
        ["offline chat"]
    );
}

#[test]
fn a_batch_edit_leaves_the_comments_alone_too() {
    let text = "\
[chat.apps.WhatsApp.reply]
# only one-to-one chats belong here
rooms = [
    \"Чех\",  # verified one-to-one
]
";
    let out = set_array_members(
        text,
        "chat.apps.WhatsApp.reply.rooms",
        &["Чех", "122 ОБЗ"],
        true,
    )
    .unwrap();
    assert!(out.contains("only one-to-one chats belong here"), "{out}");
    assert!(out.contains("# verified one-to-one"), "{out}");
    assert!(out.contains("122 ОБЗ"), "{out}");
}

// ── Repeating groups ────────────────────────────────────────────────

/// Two scheduled messages, with the comments a user would have left.
const SCHEDULE: &str = "\
[schedule]
enabled = true

# the morning one
[[schedule.sends]]
name = \"Ранкове\"
room = \"Бронза\"   # the work room
when = \"weekdays 09:00\"
enabled = true

[[schedule.sends]]
name = \"Пʼятниця\"
room = \"Бронза\"
when = \"fri 17:30\"
enabled = false
";

#[test]
fn rows_are_counted_and_read_field_by_field() {
    assert_eq!(count_records(SCHEDULE, "schedule.sends"), 2);
    assert_eq!(
        read_record_field(SCHEDULE, "schedule.sends", 0, "name"),
        Some(SettingValue::Text("Ранкове".to_owned()))
    );
    assert_eq!(
        read_record_field(SCHEDULE, "schedule.sends", 1, "enabled"),
        Some(SettingValue::Bool(false))
    );
    // A field the row omits reads as absent, not as a default we invented.
    assert_eq!(
        read_record_field(SCHEDULE, "schedule.sends", 0, "text"),
        None
    );
    // So does a row that is not there.
    assert_eq!(
        read_record_field(SCHEDULE, "schedule.sends", 9, "name"),
        None
    );
    // And a key with no array at all is no rows, not an error.
    assert_eq!(count_records(SCHEDULE, "schedule.nothing"), 0);
}

#[test]
fn editing_one_field_leaves_the_other_rows_and_the_comments_alone() {
    let out = write_record_field(
        SCHEDULE,
        "schedule.sends",
        0,
        "when",
        &SettingValue::Text("weekdays 08:45".to_owned()),
    )
    .unwrap();
    assert_eq!(
        read_record_field(&out, "schedule.sends", 0, "when"),
        Some(SettingValue::Text("weekdays 08:45".to_owned()))
    );
    // The second entry is untouched…
    assert_eq!(
        read_record_field(&out, "schedule.sends", 1, "when"),
        Some(SettingValue::Text("fri 17:30".to_owned()))
    );
    // …and so is everything the user wrote around them.
    assert!(out.contains("# the morning one"), "{out}");
    assert!(out.contains("# the work room"), "{out}");
}

#[test]
fn a_field_the_row_does_not_have_yet_is_added_to_it() {
    let out = write_record_field(
        SCHEDULE,
        "schedule.sends",
        0,
        "text",
        &SettingValue::Text("Доброго ранку!".to_owned()),
    )
    .unwrap();
    assert_eq!(
        read_record_field(&out, "schedule.sends", 0, "text"),
        Some(SettingValue::Text("Доброго ранку!".to_owned()))
    );
    assert_eq!(count_records(&out, "schedule.sends"), 2);
}

#[test]
fn writing_past_the_end_is_refused_rather_than_padded() {
    // Reaching row 5 of a two-row array by growing it would invent three
    // empty scheduled messages. The file changed underneath the pane, and
    // saying so is the only honest answer.
    let e = write_record_field(
        SCHEDULE,
        "schedule.sends",
        5,
        "name",
        &SettingValue::Text("x".to_owned()),
    );
    assert!(e.is_err());
    assert_eq!(count_records(SCHEDULE, "schedule.sends"), 2);
}

#[test]
fn a_row_is_added_empty_and_the_plug_ins_own_defaults_apply() {
    let out = add_record(SCHEDULE, "schedule.sends").unwrap();
    assert_eq!(count_records(&out, "schedule.sends"), 3);
    // Nothing invented in it: guessing at an hour would schedule a
    // message nobody chose the time of.
    assert_eq!(read_record_field(&out, "schedule.sends", 2, "when"), None);
    assert!(out.contains("# the morning one"), "{out}");
}

#[test]
fn the_first_row_can_be_added_where_there_is_no_array_yet() {
    let out = add_record("[schedule]\nenabled = false\n", "schedule.sends").unwrap();
    assert_eq!(count_records(&out, "schedule.sends"), 1);
}

#[test]
fn removing_a_row_takes_that_one_and_leaves_the_rest() {
    let out = remove_record(SCHEDULE, "schedule.sends", 0).unwrap();
    assert_eq!(count_records(&out, "schedule.sends"), 1);
    assert_eq!(
        read_record_field(&out, "schedule.sends", 0, "name"),
        Some(SettingValue::Text("Пʼятниця".to_owned()))
    );
}

#[test]
fn removing_a_row_that_is_not_there_changes_nothing() {
    // The pane and the file can disagree for a moment, and "this one is
    // gone" is already satisfied.
    let out = remove_record(SCHEDULE, "schedule.sends", 7).unwrap();
    assert_eq!(count_records(&out, "schedule.sends"), 2);
}
