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

// ── XQueryKeymap reconciliation (issue #26) ─────────────────────────

/// Build an `XQueryKeymap` reply in which exactly these evdev keys are
/// down, the way the server packs it: keycode `k` in bit `k % 8` of
/// byte `k / 8`.
fn keymap_with(down: &[u32]) -> [u8; 32] {
    let mut keys = [0u8; 32];
    for &evdev in down {
        let code = evdev_to_x11(evdev).unwrap_or_default();
        assert_ne!(code, 0, "evdev {evdev} must fit an X11 keycode");
        keys[usize::from(code) / 8] |= 1 << (code % 8);
    }
    keys
}

#[test]
fn keymap_bits_are_read_the_way_the_server_packs_them() {
    let keys = keymap_with(&[EV_LEFTALT]);
    let alt = evdev_to_x11(EV_LEFTALT).unwrap_or_default();
    assert_ne!(alt, 0, "left Alt must fit an X11 keycode");
    assert!(keycode_is_down(&keys, alt));
    // Neighbours in the same byte must not read as down — that is the
    // shape a shift-by-one bug takes.
    assert!(!keycode_is_down(&keys, alt.wrapping_add(1)));
    assert!(!keycode_is_down(&keys, alt.wrapping_sub(1)));
    assert!(!keycode_is_down(&keymap_with(&[]), alt));
}

#[test]
fn a_modifier_whose_release_we_never_saw_is_cleared() {
    // Issue #26 in one test. Cinnamon grabs the keyboard for a
    // layout-switch shortcut bound to a bare Alt; the press reaches us
    // and the release, delivered during the grab, does not. Left
    // uncorrected the engine reads every later keystroke as a shortcut
    // and the app goes quiet until it is restarted.
    let mut m = ModState::default();
    m.press(EV_RIGHTALT);
    assert!(m.snapshot().is_command(), "alt latched on the press edge");

    // The user is not holding anything: the server says so.
    assert!(m.resync(&keymap_with(&[])), "resync must report the fix");
    assert!(
        !m.snapshot().is_command(),
        "a stuck modifier must not survive a resync"
    );
}

#[test]
fn a_modifier_the_user_really_is_holding_survives() {
    let mut m = ModState::default();
    m.press(EV_LEFTALT);
    assert!(!m.resync(&keymap_with(&[EV_LEFTALT])), "nothing changed");
    assert!(m.snapshot().alt, "Alt is genuinely down — leave it alone");
}

#[test]
fn resync_notices_a_press_we_missed_as_well_as_a_release() {
    // The grab swallows edges in both directions; a modifier pressed
    // during one is just as wrong as one released during one.
    let mut m = ModState::default();
    assert!(m.resync(&keymap_with(&[EV_LEFTCTRL, EV_RIGHTSHIFT])));
    let s = m.snapshot();
    assert!(s.control && s.shift && !s.alt && !s.meta);
}

#[test]
fn either_side_of_a_modifier_pair_counts_as_held() {
    for (left, right) in [
        (EV_LEFTSHIFT, EV_RIGHTSHIFT),
        (EV_LEFTCTRL, EV_RIGHTCTRL),
        (EV_LEFTALT, EV_RIGHTALT),
        (EV_LEFTMETA, EV_RIGHTMETA),
    ] {
        for code in [left, right] {
            let mut m = ModState::default();
            m.resync(&keymap_with(&[code]));
            assert!(
                m.any_held(),
                "evdev {code} must register as a held modifier"
            );
        }
    }
}

#[test]
fn caps_lock_is_a_latch_and_resync_must_not_touch_it() {
    // Caps is toggled on the press edge and stays on with the key up.
    // XQueryKeymap reports the physical key, so folding it in here
    // would clear the latch the moment the user let go.
    let mut m = ModState::default();
    m.press(EV_CAPSLOCK);
    assert!(m.snapshot().shift, "caps folds into shift for the engine");
    m.resync(&keymap_with(&[]));
    assert!(
        m.snapshot().shift,
        "caps must survive a resync with no keys down"
    );
}

#[test]
fn an_idle_keyboard_needs_no_resync_at_all() {
    // `any_held` is the gate that keeps this off the hot path: with no
    // modifier believed down we never ask the server anything.
    assert!(!ModState::default().any_held());
    let mut m = ModState::default();
    m.press(EV_LEFTALT);
    assert!(m.any_held());
    m.release(EV_LEFTALT);
    assert!(!m.any_held());
}
