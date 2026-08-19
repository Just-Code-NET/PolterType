use super::parse::layout_short_names;

/// Verbatim `qdbus6 --literal … getLayoutsList` on Plasma 6 with
/// `us` + `ru` configured. Shape confirmed against Qt's
/// `argumentToString` (`qtbase/src/dbus/qdbusutil.cpp`) and KWin's
/// `LayoutNames` struct (`src/keyboard_layout.h`).
const PLASMA6: &str = r#"[Argument: a(sss) {[Argument: (sss) "us", "", "English (US)"], [Argument: (sss) "ru", "", "Russian"]}]"#;

#[test]
fn reads_plasma6_struct_array() {
    assert_eq!(layout_short_names(PLASMA6), ["us", "ru"]);
}

/// Pre-5.23 Plasma answered a plain `as`.
#[test]
fn reads_legacy_string_array() {
    assert_eq!(
        layout_short_names(r#"{"us", "ua", "de"}"#),
        ["us", "ua", "de"]
    );
}

/// A display name is the *second* field, so it must never be mistaken
/// for a layout — including when it repeats a short name.
#[test]
fn takes_only_the_short_name_of_each_struct() {
    let raw = r#"[Argument: a(sss) {[Argument: (sss) "ua", "us", "Ukrainian"]}]"#;
    assert_eq!(layout_short_names(raw), ["ua"]);
}

/// The bug from #31: plain `qdbus` prints this to **stdout** and exits
/// 0. Nothing parses out of it, which is what makes the caller treat
/// the backend as unusable instead of inventing a layout id.
#[test]
fn refuses_the_undisplayable_type_message() {
    let raw = "qdbus: I don't know how to display an argument of type 'a(sss)', \
               run with --literal.";
    assert!(layout_short_names(raw).is_empty());
}

#[test]
fn handles_empty_and_garbage() {
    assert!(layout_short_names("").is_empty());
    assert!(layout_short_names("[Argument: a(sss) {}]").is_empty());
    assert!(layout_short_names("nothing quoted here").is_empty());
}
