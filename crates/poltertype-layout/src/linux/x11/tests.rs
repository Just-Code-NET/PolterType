//! Pure-function tests. No X server required.

use super::consts::*;
use super::xkb::*;
use crate::LayoutId;

/// Build a `_XKB_RULES_NAMES` value the way the X server writes it.
fn rules_names(layout: &str) -> Vec<u8> {
    let fields = ["evdev", "pc105", layout, "", ""];
    fields.join("\0").into_bytes()
}

#[test]
fn parses_the_layout_list_into_bcp47() {
    let raw = rules_names("us,ua");
    assert_eq!(
        parse_rules_names(&raw),
        vec![LayoutId::new("en-US"), LayoutId::new("uk-UA")]
    );
}

#[test]
fn single_layout_parses() {
    assert_eq!(
        parse_rules_names(&rules_names("de")),
        vec![LayoutId::new("de-DE")]
    );
}

#[test]
fn unknown_xkb_codes_pass_through_untranslated() {
    // A layout we have no BCP-47 mapping for must still be listed —
    // otherwise its index shifts and every *other* layout resolves to
    // the wrong one.
    let parsed = parse_rules_names(&rules_names("us,zz,ua"));
    assert_eq!(
        parsed,
        vec![
            LayoutId::new("en-US"),
            LayoutId::new("zz"),
            LayoutId::new("uk-UA"),
        ]
    );
}

#[test]
fn empty_and_malformed_properties_yield_no_layouts() {
    assert!(parse_rules_names(b"").is_empty());
    // Truncated: no layout field at all.
    assert!(parse_rules_names(b"evdev\0pc105").is_empty());
    // Present but blank.
    assert!(parse_rules_names(&rules_names("")).is_empty());
}

#[test]
fn trailing_commas_do_not_produce_phantom_layouts() {
    // `setxkbmap -layout us,` is legal and leaves an empty field.
    assert_eq!(
        parse_rules_names(&rules_names("us,")),
        vec![LayoutId::new("en-US")]
    );
}

#[test]
fn group_index_maps_to_xkb_group_and_stops_at_four() {
    for idx in 0..MAX_GROUPS {
        assert_eq!(
            group_from_index(idx).ok().map(|g| usize::from(u8::from(g))),
            Some(idx),
            "group index {idx} must map to the XKB group of the same number"
        );
    }
    assert!(group_from_index(MAX_GROUPS).is_err());
}
