//! Host-runnable tests for the macOS keyboard facts.
//!
//! These run on Linux and Windows CI as well as on a Mac — the module
//! under test has no Apple dependency. That is the whole point: the
//! table and the direction rules are where a macOS mistake is silent
//! (a wrong scancode drops a word without any error), and a Mac is the
//! one machine this project does not have.

use super::codes::*;
use crate::KeyDirection;

/// Scancodes the engine's buffer classifier treats as "a modifier
/// moved — ignore it but stay inside the word"
/// (`poltertype-core/src/engine/buffer/classify.rs`).
const DISCARD: &[u32] = &[0x1D, 0x2A, 0x36, 0x38, 0x3A];

/// The range the classifier reads as "navigation / function key — end
/// the word and throw it away".
const END_AND_DISCARD: std::ops::RangeInclusive<u32> = 0x3B..=0x53;

#[test]
fn every_modifier_maps_into_the_classifier_discard_set() {
    for kvk in [
        KVK_SHIFT,
        KVK_RIGHT_SHIFT,
        KVK_CONTROL,
        KVK_RIGHT_CONTROL,
        KVK_OPTION,
        KVK_RIGHT_OPTION,
        KVK_CAPS_LOCK,
    ] {
        let sc = mac_keycode_to_sc1(kvk);
        assert!(
            DISCARD.contains(&sc),
            "Apple keycode {kvk:#04x} maps to SC-1 {sc:#04x}, which is not a modifier \
             the classifier ignores — a word typed with this key held would be lost"
        );
    }
}

#[test]
fn command_keys_map_to_the_win_key_slots() {
    // SC-1 0x5B / 0x5C sit outside every classifier range, so they fall
    // through to "no character produced → Discard" exactly as LWin and
    // RWin do on Windows. Asserted separately from the modifiers above
    // because they get there by a different route.
    assert_eq!(mac_keycode_to_sc1(KVK_COMMAND), 0x5B);
    assert_eq!(mac_keycode_to_sc1(KVK_RIGHT_COMMAND), 0x5C);
    for sc in [0x5B, 0x5C] {
        assert!(!END_AND_DISCARD.contains(&sc));
        assert_ne!(sc, 0x39, "must not alias onto Space");
    }
}

#[test]
fn no_letter_or_digit_lands_in_a_control_range() {
    // The Apple keycodes for the alphanumeric block. If any of them
    // translated into the classifier's control ranges, typing that
    // letter would end the word instead of extending it.
    let letters_and_digits: &[u16] = &[
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x1F, 0x20, 0x22, 0x23, 0x25, 0x26, 0x28, 0x2D, 0x2E, 0x12, 0x13, 0x14, 0x15,
        0x16, 0x17, 0x19, 0x1A, 0x1C, 0x1D,
    ];
    for &kvk in letters_and_digits {
        let sc = mac_keycode_to_sc1(kvk);
        assert!(
            !END_AND_DISCARD.contains(&sc) && !DISCARD.contains(&sc),
            "Apple keycode {kvk:#04x} maps to SC-1 {sc:#04x}, inside a control range"
        );
        assert_ne!(sc, 0x39, "Apple keycode {kvk:#04x} aliases onto Space");
        assert_ne!(sc, 0x0E, "Apple keycode {kvk:#04x} aliases onto Backspace");
    }
}

#[test]
fn boundary_keys_keep_their_meaning() {
    assert_eq!(mac_keycode_to_sc1(0x31), 0x39, "Space");
    assert_eq!(mac_keycode_to_sc1(0x24), 0x1C, "Return");
    assert_eq!(mac_keycode_to_sc1(0x4C), 0x1C, "Numpad Enter");
    assert_eq!(mac_keycode_to_sc1(0x30), 0x0F, "Tab");
    assert_eq!(mac_keycode_to_sc1(KVK_DELETE), 0x0E, "Backspace");
    assert_eq!(mac_keycode_to_sc1(0x35), 0x01, "Esc");
}

#[test]
fn flags_changed_reads_the_post_change_state() {
    // Shift going down: its bit is set in the flags that come with the
    // event. Going up: the bit is gone.
    assert_eq!(
        flags_changed_direction(KVK_SHIFT, FLAG_SHIFT),
        Some(KeyDirection::Press)
    );
    assert_eq!(
        flags_changed_direction(KVK_SHIFT, 0),
        Some(KeyDirection::Release)
    );
    // Right-hand keys share the device-independent bit.
    assert_eq!(
        flags_changed_direction(KVK_RIGHT_SHIFT, FLAG_SHIFT),
        Some(KeyDirection::Press)
    );
    // Releasing Control while Shift stays down: only Control's bit is
    // consulted, so the still-held Shift must not read as a press.
    assert_eq!(
        flags_changed_direction(KVK_CONTROL, FLAG_SHIFT),
        Some(KeyDirection::Release)
    );
    assert_eq!(
        flags_changed_direction(KVK_CONTROL, FLAG_SHIFT | FLAG_CONTROL),
        Some(KeyDirection::Press)
    );
    assert_eq!(
        flags_changed_direction(KVK_OPTION, FLAG_ALTERNATE),
        Some(KeyDirection::Press)
    );
    assert_eq!(
        flags_changed_direction(KVK_COMMAND, FLAG_COMMAND),
        Some(KeyDirection::Press)
    );
}

#[test]
fn caps_lock_latch_reads_as_press_then_release() {
    assert_eq!(
        flags_changed_direction(KVK_CAPS_LOCK, FLAG_ALPHA_SHIFT),
        Some(KeyDirection::Press)
    );
    assert_eq!(
        flags_changed_direction(KVK_CAPS_LOCK, 0),
        Some(KeyDirection::Release)
    );
}

#[test]
fn untracked_modifier_keys_are_dropped_not_mistranslated() {
    // Fn (0x3F) and the media keys have no SC-1 equivalent. The
    // identity fallback would put Fn at SC-1 0x3F — inside
    // `0x3B..=0x53`, i.e. "end the word and discard it" — so holding Fn
    // to reach an arrow key would silently eat whatever was typed.
    // Returning `None` is what keeps the listener from emitting them.
    const KVK_FUNCTION: u16 = 0x3F;
    assert!(END_AND_DISCARD.contains(&mac_keycode_to_sc1(KVK_FUNCTION)));
    assert_eq!(flags_changed_direction(KVK_FUNCTION, 0), None);
    assert_eq!(flags_changed_direction(0x4F, 0), None);
}

/// The flag bits are transcribed from Apple's header so the table can
/// compile off-Mac; on a Mac we can hold the transcription against the
/// real thing.
#[cfg(target_os = "macos")]
#[test]
fn flag_constants_match_core_graphics() {
    use core_graphics::event::CGEventFlags;
    assert_eq!(FLAG_ALPHA_SHIFT, CGEventFlags::CGEventFlagAlphaShift.bits());
    assert_eq!(FLAG_SHIFT, CGEventFlags::CGEventFlagShift.bits());
    assert_eq!(FLAG_CONTROL, CGEventFlags::CGEventFlagControl.bits());
    assert_eq!(FLAG_ALTERNATE, CGEventFlags::CGEventFlagAlternate.bits());
    assert_eq!(FLAG_COMMAND, CGEventFlags::CGEventFlagCommand.bits());
}

#[test]
fn sc1_to_mac_keycode_round_trips_through_the_forward_table() {
    // The reverse table exists for `send_chord`; a mismatch with the
    // forward table means the chord we post is not the key the engine
    // named. Walk the whole SC-1 main block: wherever the reverse map
    // answers, the forward map must take that keycode straight back.
    let mut covered = 0;
    for sc1 in 0x01..=0x39u32 {
        if let Some(kvk) = sc1_to_mac_keycode(sc1) {
            assert_eq!(
                mac_keycode_to_sc1(kvk),
                sc1,
                "SC-1 {sc1:#04x} → Apple {kvk:#04x} does not round-trip"
            );
            covered += 1;
        }
    }
    // The main block minus the keys a chord cannot name (modifiers,
    // Caps Lock): the count catches a silently shrunken table.
    assert_eq!(covered, 52);
}

#[test]
fn chord_keys_the_engine_actually_presses_are_mapped() {
    // The two chords the engine sends today. If either ever answers
    // `None`, selection conversion dies at runtime with an
    // `Unsupported` — this pins it at test time instead.
    assert_eq!(sc1_to_mac_keycode(0x2E), Some(0x08)); // C — the copy chord
    assert_eq!(sc1_to_mac_keycode(0x2F), Some(0x09)); // V — the paste chord
}

#[test]
fn sc1_to_mac_keycode_refuses_what_it_cannot_name() {
    // No identity fallback: SC-1 and Apple keycodes overlap with
    // different meanings, and a wrong key pressed with Cmd held is a
    // real action in someone's application.
    for sc1 in [0x1D, 0x2A, 0x36, 0x38, 0x3A, 0x3B, 0x48, 0x5B, 0xFF] {
        assert_eq!(sc1_to_mac_keycode(sc1), None, "SC-1 {sc1:#04x}");
    }
}
