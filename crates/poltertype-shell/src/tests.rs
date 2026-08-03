//! Unit tests for the shell quirks.
//!
//! Each assertion is written twice, once per branch of the platform
//! it describes, so the test says what the *other* platforms do
//! rather than silently skipping them. The behaviours here are all
//! ones that failed quietly in the field: a lock id that was a name
//! where a path was wanted, and key names the user could not map to
//! their keyboard.

use crate::{acquire_instance_lock, key_glyph, key_name_with_glyph};

#[test]
fn the_lock_admits_one_holder_and_releases_on_drop() {
    // The property the whole thing exists for. Deliberately exercised
    // through the real primitive rather than a stand-in: what broke
    // before was the primitive's own behaviour, not our logic about it.
    let id = format!("dev.opensource.poltertype-test-{}", std::process::id());
    let dir = std::env::temp_dir().join(&id);

    let first = acquire_instance_lock(&id, &dir).expect("first acquire failed");
    assert!(first.is_some(), "nothing else should hold a per-pid name");

    let second = acquire_instance_lock(&id, &dir).expect("second acquire errored");
    assert!(
        second.is_none(),
        "a second holder must be refused, not granted"
    );

    // Releasing must actually release — otherwise the first crash of
    // the day locks the user out until they reboot.
    drop(first);
    let third = acquire_instance_lock(&id, &dir).expect("third acquire failed");
    assert!(third.is_some(), "the lock did not come back after release");

    drop(third);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_locks_an_absolute_path_rather_than_a_bare_name() {
    // macOS `flock`s the id as a FILE, so a bare name lands in the
    // process working directory — which is `/` under Finder and
    // launchd, and read-only. v0.5.0 aborted at startup there.
    let dir = std::path::Path::new("/tmp/pt-test-cfg");
    let id = crate::instance::lock_id("dev.opensource.poltertype", dir);
    assert!(std::path::Path::new(&id).is_absolute(), "{id}");
    assert!(id.ends_with("dev.opensource.poltertype.lock"), "{id}");
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
