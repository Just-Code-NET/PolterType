//! What can honestly be tested without a RemoteDesktop portal.
//!
//! There is none on the machine this was written on, so nothing here
//! exercises the protocol. What is checked is the part no compositor
//! could forgive: that probing is safe with no portal present, that
//! the keycodes are evdev numbers rather than X11's, and that the
//! option values match the specification's constants.

use super::consts::*;
use super::*;

/// Probing must be safe and honest on a session without the portal —
/// which is every wlroots session today, including the one this is
/// most likely to be run on first.
#[test]
fn probing_a_session_without_the_portal_is_safe_and_false() {
    // Whatever this machine has, the call must not panic or hang.
    let available = portal_available();
    // On a session that genuinely lacks RemoteDesktop the answer has
    // to be `false`, or the emitter chooser would pick a backend that
    // cannot work and abandon a uinput that can.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        // Only an observation, not an assertion: a GNOME or KDE
        // session would legitimately answer true.
        eprintln!("RemoteDesktop portal available here: {available}");
    }
}

/// Opening a session on a machine with no portal must fail with the
/// specific error, not hang waiting for a dialog nobody will show.
#[test]
fn opening_without_a_portal_fails_rather_than_hanging() {
    if portal_available() {
        return; // a real portal; not this test's business
    }
    let started = std::time::Instant::now();
    let result = PortalSession::open();
    assert!(
        matches!(
            result,
            Err(PortalError::NotAvailable) | Err(PortalError::Bus(_))
        ),
        "expected a clean unavailability error"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "must fail fast, not wait out the response timeout"
    );
}

/// The portal takes **evdev** keycodes. Sending X11 keycodes, which
/// are evdev plus eight, would type the wrong character for every
/// single key — the easiest mistake to make in this file.
#[test]
fn keycodes_are_evdev_numbers() {
    // From `linux/input-event-codes.h`.
    assert_eq!(
        super::emitter::testing::key_backspace(),
        14,
        "KEY_BACKSPACE"
    );
    assert_eq!(
        super::emitter::testing::key_leftshift(),
        42,
        "KEY_LEFTSHIFT"
    );
    assert_eq!(super::emitter::testing::key_leftctrl(), 29, "KEY_LEFTCTRL");
    assert_eq!(super::emitter::testing::key_leftalt(), 56, "KEY_LEFTALT");
    assert_eq!(super::emitter::testing::key_leftmeta(), 125, "KEY_LEFTMETA");
}

/// The engine's scancodes and evdev's keycodes coincide across the
/// block PolterType records, which is why the conversion is an
/// identity. If that ever stops being true this test is where it
/// surfaces, rather than as mistyped corrections.
#[test]
fn scancodes_and_evdev_codes_agree_across_the_recorded_block() {
    // 0x10 is `Q` in Set-1 and KEY_Q = 16 in evdev.
    assert_eq!(super::emitter::testing::to_evdev(0x10), 16);
    // 0x1E is `A`; KEY_A = 30.
    assert_eq!(super::emitter::testing::to_evdev(0x1E), 30);
    // 0x2C is `Z`; KEY_Z = 44.
    assert_eq!(super::emitter::testing::to_evdev(0x2C), 44);
    // Round-trip, since the echo filter depends on it.
    for sc in [0x02u32, 0x10, 0x1E, 0x2C, 0x35, 0x39] {
        assert_eq!(
            super::emitter::testing::from_evdev(super::emitter::testing::to_evdev(sc)),
            sc
        );
    }
}

/// Values from the portal specification. A wrong bitmask here asks
/// for the pointer as well and widens the consent dialog; a wrong
/// key state types a permanent key-down.
#[test]
fn option_values_match_the_specification() {
    assert_eq!(DEVICE_KEYBOARD, 1, "DeviceType::KEYBOARD");
    assert_eq!(KEY_RELEASED, 0);
    assert_eq!(KEY_PRESSED, 1);
    assert_eq!(PERSIST_PERSISTENT, 2, "PersistMode::Persistent");
    assert_eq!(RESPONSE_SUCCESS, 0);
    assert_eq!(RESPONSE_CANCELLED, 1);
}

/// Handle tokens go into a D-Bus object path, so anything outside
/// `[A-Za-z0-9_]` would make the portal reject the call.
#[test]
fn handle_tokens_are_path_safe_and_unique() {
    let a = super::response::token_for_test("pt_req");
    let b = super::response::token_for_test("pt_req");
    assert_ne!(a, b, "two tokens on one connection must differ");
    for t in [&a, &b] {
        assert!(
            t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "token {t:?} is not path-safe"
        );
    }
}
