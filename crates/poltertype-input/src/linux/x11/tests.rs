//! Pure-function tests. No X server required.

use super::codes::*;
use super::consts::*;
use super::types::*;

#[test]
fn keycode_round_trips_through_the_evdev_offset() {
    // KEY_A is evdev 30, which every Linux XKB keymap exposes as X11
    // keycode 38 — the single fact the whole backend rests on.
    assert_eq!(x11_to_evdev(38), Some(30));
    assert_eq!(evdev_to_x11(30), Some(38));

    for evdev in 0u32..=247 {
        assert_eq!(
            evdev_to_x11(evdev).and_then(|kc| x11_to_evdev(u32::from(kc))),
            Some(evdev),
            "evdev {evdev} must survive the round trip"
        );
    }
}

#[test]
fn x11_reserved_keycodes_are_dropped() {
    // 0-7 are the X protocol's reserved range: no physical key ever
    // reports them, and subtracting the offset would wrap.
    for kc in 0u32..EVDEV_OFFSET {
        assert_eq!(x11_to_evdev(kc), None, "keycode {kc} should be dropped");
    }
    assert_eq!(x11_to_evdev(EVDEV_OFFSET), Some(0));
}

#[test]
fn evdev_codes_past_the_x11_wire_width_are_rejected() {
    // X11 keycodes are a single byte, so evdev 248+ has nowhere to go.
    assert_eq!(evdev_to_x11(247), Some(255));
    assert_eq!(evdev_to_x11(248), None);
}

#[test]
fn scroll_wheel_is_not_a_caret_jump() {
    for button in 1..=3 {
        assert!(is_caret_jump_button(button), "button {button} moves caret");
    }
    // 4-7 are wheel up/down/left/right — scrolling leaves the caret be.
    for button in 4..=7 {
        assert!(!is_caret_jump_button(button), "button {button} is scroll");
    }
    assert!(is_caret_jump_button(8));
    assert!(is_caret_jump_button(9));
}

#[test]
fn latin1_maps_to_itself_and_the_rest_gets_bit_24() {
    assert_eq!(unicode_to_keysym('a'), 0x61);
    assert_eq!(unicode_to_keysym('ÿ'), 0xFF);
    // Cyrillic д — the payload case for a uk-UA correction.
    assert_eq!(unicode_to_keysym('д'), 0x0100_0434);
    assert_eq!(unicode_to_keysym('€'), 0x0100_20AC);
}

#[test]
fn caps_lock_folds_into_shift() {
    let mut m = ModState::default();
    assert!(!m.snapshot().shift);

    m.press(EV_LEFTSHIFT);
    assert!(m.snapshot().shift);
    m.release(EV_LEFTSHIFT);
    assert!(!m.snapshot().shift);

    // Caps toggles on press and stays on after release.
    m.press(EV_CAPSLOCK);
    m.release(EV_CAPSLOCK);
    assert!(m.snapshot().shift, "caps lock alone produces uppercase");

    // Shift while Caps is on cancels back to lowercase.
    m.press(EV_LEFTSHIFT);
    assert!(!m.snapshot().shift, "shift+caps is lowercase");

    m.release(EV_LEFTSHIFT);
    m.press(EV_CAPSLOCK);
    assert!(!m.snapshot().shift, "caps toggled back off");
}

#[test]
fn modifiers_are_tracked_independently() {
    let mut m = ModState::default();
    m.press(EV_LEFTCTRL);
    m.press(EV_RIGHTALT);
    let s = m.snapshot();
    assert!(s.control && s.alt);
    assert!(!s.shift && !s.meta);

    m.release(EV_LEFTCTRL);
    assert!(!m.snapshot().control);
    assert!(m.snapshot().alt, "releasing ctrl must not clear alt");
}

#[test]
fn every_tracked_modifier_is_recognised() {
    for code in [
        EV_LEFTSHIFT,
        EV_RIGHTSHIFT,
        EV_LEFTCTRL,
        EV_RIGHTCTRL,
        EV_LEFTALT,
        EV_RIGHTALT,
        EV_LEFTMETA,
        EV_RIGHTMETA,
        EV_CAPSLOCK,
    ] {
        assert!(is_modifier(code), "evdev {code} is a modifier");
    }
    // KEY_A must not be.
    assert!(!is_modifier(30));
}
