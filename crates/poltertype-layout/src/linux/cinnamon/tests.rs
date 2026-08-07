//! Pure-function tests. No session bus, no X server.

use super::*;
use crate::LayoutId;

/// A reply in the shape `gdbus call` prints it: the method's single
/// out-argument wrapped in a one-element tuple, no trailing newline
/// worth caring about.
const TWO_SOURCES: &str = "([('xkb', 'us', 0, 'English (US)', 'en', 'us', 'us', 'us', '', '', -1, true), \
     ('xkb', 'ru', 1, 'Russian', 'ru', 'ru', 'ru', 'ru', '', '', -1, false)],)\n";

#[test]
fn reads_the_layouts_and_which_one_is_current() {
    let sources = parse_input_sources(TWO_SOURCES);
    assert_eq!(
        sources.iter().map(|s| s.layout.clone()).collect::<Vec<_>>(),
        vec![LayoutId::new("en-US"), LayoutId::new("ru-RU")]
    );
    assert_eq!(
        sources.iter().map(|s| s.is_current).collect::<Vec<_>>(),
        vec![true, false]
    );
}

#[test]
fn keeps_cinnamons_own_index_rather_than_the_position_in_our_list() {
    // Captured verbatim from `gdbus call … GetInputSources` against a
    // service exporting the real signature. What
    // `ActivateInputSourceIndex` is given must be the index Cinnamon
    // reported: the IBus engine at 0 has no XKB layout behind it, so
    // it is not switchable and drops out of our list — and if we then
    // sent a *list position* the user would land in the wrong layout.
    let raw = "([('ibus', 'anthy', 0, 'Japanese (Anthy)', 'JA', 'anthy', '', '', '', '', -1, false), \
         ('xkb', 'us', 1, 'English (US)', 'en', 'us', 'us', 'us', '', '', -1, true), \
         ('xkb', 'ru', 2, \"Hawai'ian? no — Russian, (RU)\", 'ru', 'ru', 'ru', 'ru', '', '', -1, false)],)\n";
    let sources = parse_input_sources(raw);
    assert_eq!(
        sources.iter().map(|s| s.layout.clone()).collect::<Vec<_>>(),
        vec![LayoutId::new("en-US"), LayoutId::new("ru-RU")]
    );
    assert_eq!(
        sources.iter().map(|s| s.index).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(sources[0].is_current);
}

#[test]
fn an_ibus_engine_with_an_xkb_layout_is_switchable() {
    // `engineDesc.get_layout()` fills the same field for IBus sources,
    // and when it names a real layout the source is one we can pick.
    let raw = "([('ibus', 'xkb:de::ger', 0, 'German', 'de', 'de', 'de', 'de', '', '', -1, true)],)";
    let sources = parse_input_sources(raw);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].layout, LayoutId::new("de-DE"));
}

#[test]
fn quoting_inside_a_display_name_does_not_split_the_tuple() {
    // Verbatim `g_variant_print` output for four sources whose display
    // names carry commas, parentheses, backslashes and both kinds of
    // quote. The apostrophe cases are the ones that matter: glib does
    // not escape `'`, it re-quotes the whole literal with `"`, and a
    // scanner that assumes single quotes loses the rest of the reply.
    let raw = "([('xkb', 'us', 0, 'English (US)', 'en', 'us', 'us', 'us', '', '', -1, true), \
         ('xkb', 'ru', 1, 'Russian', 'ru', 'ru', 'ru', 'ru', '', '', -1, false), \
         ('xkb', 'us', 2, \"Hawai'ian (US, alt), \\\\o/\", 'haw', 'us', 'us', 'us', 'alt-hawaii', '', -1, false), \
         ('xkb', 'de', 3, \"It's a \\\"quoted\\\" name\", 'de', 'de', 'de', 'de', '', '', -1, false)],)";
    let sources = parse_input_sources(raw);
    assert_eq!(
        sources.iter().map(|s| s.layout.clone()).collect::<Vec<_>>(),
        vec![
            LayoutId::new("en-US"),
            LayoutId::new("ru-RU"),
            LayoutId::new("en-US"),
            LayoutId::new("de-DE"),
        ]
    );
    assert_eq!(sources.iter().filter(|s| s.is_current).count(), 1);
    assert_eq!(
        sources.iter().map(|s| s.index).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn a_layout_we_have_no_bcp47_name_for_passes_through() {
    let raw = "([('xkb', 'zz', 0, 'Nowhere', 'zz', 'zz', 'zz', 'zz', '', '', -1, true)],)";
    assert_eq!(parse_input_sources(raw)[0].layout, LayoutId::new("zz"));
}

#[test]
fn replies_we_cannot_read_yield_no_sources_rather_than_wrong_ones() {
    // Empty list, an error string, a truncated reply, and a tuple with
    // the wrong number of fields — which is what an interface change
    // would look like, and must not be read off by one.
    assert!(parse_input_sources("([],)").is_empty());
    assert!(parse_input_sources("").is_empty());
    assert!(parse_input_sources("([('xkb', 'us', 0, 'English (US").is_empty());
    assert!(parse_input_sources("([('xkb', 'us', 0, 'English (US)', 'en', 'us')],)").is_empty());
}

#[test]
fn every_spelling_of_the_session_name_is_recognised() {
    for value in [
        "X-Cinnamon",
        "X-Cinnamon:Cinnamon",
        "cinnamon",
        "CINNAMON",
        "cinnamon2d",
        // Some display managers write the session file's full path.
        "/usr/share/xsessions/cinnamon",
    ] {
        assert!(
            names_cinnamon(value),
            "{value} should name a Cinnamon session"
        );
    }
}

#[test]
fn other_desktops_are_left_to_their_own_backends() {
    for value in [
        "",
        "GNOME",
        "ubuntu:GNOME",
        "KDE",
        "XFCE",
        "MATE",
        "Pantheon",
    ] {
        assert!(!names_cinnamon(value), "{value} is not Cinnamon");
    }
}

#[test]
fn a_fork_that_merely_starts_with_the_name_is_not_claimed() {
    // Entries are matched whole. We know Cinnamon's input stack; we
    // know nothing about a derivative's, and claiming it on a prefix
    // is how the gsettings backend got this wrong in the first place.
    assert!(!names_cinnamon("Cinnamon-Next"));
    assert!(!names_cinnamon("NotCinnamon"));
}
