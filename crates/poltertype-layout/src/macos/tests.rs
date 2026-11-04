//! The TIS calls themselves must run on the main dispatch queue, which a
//! test thread cannot reach, so these pin the key table the describer
//! walks rather than the OS answer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::consts::CHARACTER_KEYS;

/// Every scancode a bundled layout lists must be one the describer asks
/// about — a key it never asks about is a key an OS keymap silently
/// drops when it replaces the bundled table.
#[test]
fn character_keys_cover_every_bundled_scancode() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/layout-mappings/en_us.toml"
    );
    let toml = std::fs::read_to_string(path).expect("read en_us.toml");
    for line in toml.lines().filter(|l| l.starts_with("0x")) {
        let hex = line.split_whitespace().next().unwrap_or_default();
        let sc = u32::from_str_radix(hex.trim_start_matches("0x"), 16).expect("hex scancode");
        assert!(
            CHARACTER_KEYS.iter().any(|&(k, _)| k == sc),
            "0x{sc:02X} is bundled but never described"
        );
    }
}

/// One row per scancode and per Apple keycode — a duplicate would
/// describe one physical key twice under two names.
#[test]
fn character_keys_are_one_to_one() {
    let mut scancodes = std::collections::HashSet::new();
    let mut keycodes = std::collections::HashSet::new();
    for &(sc, kvk) in CHARACTER_KEYS {
        assert!(scancodes.insert(sc), "SC-1 0x{sc:02X} listed twice");
        assert!(
            keycodes.insert(kvk),
            "Apple keycode 0x{kvk:02X} listed twice"
        );
    }
}
