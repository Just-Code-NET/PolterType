//! Unit tests for the shell quirks.
//!
//! Each assertion is written twice, once per branch of the platform
//! it describes, so the test says what the *other* platforms do
//! rather than silently skipping them. The behaviours here are all
//! ones that failed quietly in the field: a lock id that was a name
//! where a path was wanted, and key names the user could not map to
//! their keyboard.

use std::path::Path;

use crate::{instance_lock_id, key_glyph, key_name_with_glyph};

#[test]
fn lock_id_is_a_path_only_where_the_crate_wants_one() {
    let id = instance_lock_id("dev.opensource.poltertype", Path::new("/tmp/pt-test-cfg"));
    if cfg!(target_os = "macos") {
        assert!(
            Path::new(&id).is_absolute(),
            "macOS flocks the id as a file; a bare name lands in cwd, which is / under Finder: {id}"
        );
        assert!(id.ends_with("dev.opensource.poltertype.lock"), "{id}");
    } else {
        assert_eq!(
            id, "dev.opensource.poltertype",
            "a named mutex / abstract socket takes the id verbatim"
        );
    }
}

#[test]
fn glyphs_are_a_macos_only_presentation() {
    if cfg!(target_os = "macos") {
        assert_eq!(key_glyph("Ctrl"), Some("⌃"));
        assert_eq!(key_glyph("meta"), Some("⌘"), "matching is case-insensitive");
        assert_eq!(key_glyph("Backspace"), Some("⌫"));
        assert_eq!(key_glyph("F9"), None, "unknown tokens keep their name");
    } else {
        assert_eq!(key_glyph("Ctrl"), None);
        assert_eq!(key_glyph("Meta"), None);
    }
}

#[test]
fn annotation_keeps_the_name_the_config_uses() {
    let annotated = key_name_with_glyph("Ctrl");
    if cfg!(target_os = "macos") {
        assert_eq!(
            annotated, "Ctrl (⌃)",
            "the glyph alone would leave the user guessing what to type in config.toml"
        );
    } else {
        assert_eq!(annotated, "Ctrl");
    }
    // `Space` maps to itself on macOS; annotating it would read
    // "Space (Space)".
    assert_eq!(key_name_with_glyph("Space"), "Space");
}
