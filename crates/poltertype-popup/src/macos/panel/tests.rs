//! The accept-hint rewrite. `[suggestions].accept_modifiers` is a
//! free-text config value, so this runs on whatever the user typed.

use super::mac_hint;

#[test]
fn modifiers_are_rewritten_in_macos_notation() {
    assert_eq!(mac_hint("Ctrl+Shift"), "⌃⇧");
    assert_eq!(mac_hint("Cmd+Alt"), "⌘⌥");
}

/// The config is case- and space-insensitive everywhere else; the
/// hint must not be the one place a stray space changes the answer.
#[test]
fn spelling_and_spacing_do_not_matter() {
    assert_eq!(mac_hint("control + OPTION"), "⌃⌥");
    assert_eq!(mac_hint("super"), "⌘");
}

/// A key that is not a modifier has no symbol to show, and half a
/// chord is more confusing than the modifiers alone.
#[test]
fn non_modifier_tokens_are_dropped() {
    assert_eq!(mac_hint("Ctrl+Shift+Q"), "⌃⇧");
    assert_eq!(mac_hint("nonsense"), "");
}
