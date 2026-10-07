//! What the tray's hover text says, and in which of its two fields.

use super::*;

/// The name and the state are separate, because the tooltip has two
/// fields and a panel lays them out — issue #59's follow-up, where the
/// whole line arrived as one long title.
#[test]
fn the_name_and_the_state_are_two_fields() {
    let (name, detail) = tooltip_for(Some(&LayoutId::from("en-US")), false, None, 0);
    assert_eq!(name, APP_NAME);
    assert_eq!(detail, "en-US");
}

/// Everything the tooltip has to say beyond the name goes in the
/// second field, in one line, however much of it there is.
#[test]
fn every_state_the_icon_cannot_show_lands_in_the_detail() {
    let (name, detail) = tooltip_for(
        Some(&LayoutId::from("uk-UA")),
        true,
        Some(InputAlert::NoHooks),
        2,
    );
    assert_eq!(name, APP_NAME);
    assert!(
        detail.starts_with("uk-UA"),
        "the layout comes first: {detail}"
    );
    assert!(detail.contains("keyboard"), "the alert is there: {detail}");
    assert!(detail.contains('2'), "the draft count is there: {detail}");
    assert!(
        !detail.contains(APP_NAME),
        "the name is not repeated: {detail}"
    );
}

/// Nothing known yet: the name stands alone, and the detail is empty
/// rather than a dangling separator — which is what tells the Linux
/// backend to send a null body.
#[test]
fn an_unknown_layout_leaves_the_detail_empty() {
    let (name, detail) = tooltip_for(None, false, None, 0);
    assert_eq!(name, APP_NAME);
    assert!(detail.is_empty(), "unexpected detail: {detail}");
}

/// Layouts switching while words stay as typed is the macOS state with
/// Input Monitoring but no Accessibility; the tooltip has to name it,
/// not claim the keyboard is unreachable (issue #76).
#[test]
fn a_correction_alert_says_words_are_not_corrected() {
    let (_, detail) = tooltip_for(
        Some(&LayoutId::from("en-US")),
        false,
        Some(InputAlert::NoCorrections),
        0,
    );
    assert!(
        detail.contains("not corrected"),
        "unexpected detail: {detail}"
    );
    assert!(!detail.contains("keyboard"), "wrong alert: {detail}");
}
