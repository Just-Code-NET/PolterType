//! Keycode / keysym translation. Pure functions — unit-tested in
//! `tests.rs` without an X server.

use super::consts::*;
use crate::ClaimKey;

/// X11 keycode → evdev keycode (which the engine treats as an SC Set-1
/// scancode; see [`EVDEV_OFFSET`]).
///
/// Keycodes below the offset are X's reserved range and never map to a
/// real key, so they're dropped rather than wrapped around.
pub(crate) fn x11_to_evdev(keycode: u32) -> Option<u32> {
    keycode.checked_sub(EVDEV_OFFSET)
}

/// evdev keycode → X11 keycode.
///
/// X11 keycodes are 8-bit on the wire; anything that doesn't fit is
/// not a key we can synthesise.
pub(crate) fn evdev_to_x11(evdev: u32) -> Option<u8> {
    u8::try_from(evdev + EVDEV_OFFSET).ok()
}

/// The claimable key an evdev code is, if any.
pub(crate) fn claim_key_for_evdev(evdev: u32) -> Option<ClaimKey> {
    match evdev {
        EV_UP => Some(ClaimKey::Up),
        EV_DOWN => Some(ClaimKey::Down),
        EV_ENTER | EV_KPENTER => Some(ClaimKey::Enter),
        EV_F1..=EV_F9 => Some(ClaimKey::Function((evdev - EV_F1 + 1) as u8)),
        EV_1..=EV_9 => Some(ClaimKey::Digit((evdev - EV_1 + 1) as u8)),
        _ => None,
    }
}

/// Every evdev code that is `key` — two for Enter, one otherwise.
pub(crate) fn evdev_codes_for(key: ClaimKey) -> Vec<u32> {
    match key {
        ClaimKey::Up => vec![EV_UP],
        ClaimKey::Down => vec![EV_DOWN],
        ClaimKey::Enter => vec![EV_ENTER, EV_KPENTER],
        ClaimKey::Function(n) => vec![EV_F1 + u32::from(n.clamp(1, 9)) - 1],
        ClaimKey::Digit(n) => vec![EV_1 + u32::from(n.clamp(1, 9)) - 1],
    }
}

/// Is this X11 keycode down, per an `XQueryKeymap` bit vector?
///
/// The reply is 32 bytes covering keycodes 0–255, least significant
/// bit first: keycode `k` lives in bit `k % 8` of byte `k / 8`.
pub(crate) fn keycode_is_down(keys: &[u8; 32], keycode: u8) -> bool {
    let (byte, bit) = (usize::from(keycode) / 8, keycode % 8);
    keys.get(byte).is_some_and(|b| b & (1 << bit) != 0)
}

/// Is this XInput2 button number a caret-moving click?
///
/// Buttons 1/2/3 and 8/9 can all move the text cursor, which the engine
/// must know about or its buffer silently diverges from the screen.
/// Buttons 4–7 are the scroll wheel: scrolling moves no caret, so
/// reporting them would abandon the buffer for nothing. The evdev
/// backend gets this free — the kernel reports scroll as `REL_WHEEL`.
pub(crate) fn is_caret_jump_button(button: u32) -> bool {
    matches!(button, 1..=3 | 8 | 9)
}

/// Unicode codepoint → X11 keysym.
///
/// Latin-1 maps to itself (a historical accident the X protocol is
/// stuck with); everything else uses the "Unicode keysym" range added
/// in X11R6.something, which is just the codepoint with bit 24 set.
///
/// Reference: <https://www.x.org/releases/current/doc/xproto/x11protocol.html>
pub(crate) fn unicode_to_keysym(c: char) -> u32 {
    let cp = c as u32;
    if cp < 0x100 { cp } else { cp | 0x0100_0000 }
}

/// Does this evdev keycode toggle or hold a modifier we track?
pub(crate) fn is_modifier(evdev: u32) -> bool {
    matches!(
        evdev,
        EV_LEFTSHIFT
            | EV_RIGHTSHIFT
            | EV_LEFTCTRL
            | EV_RIGHTCTRL
            | EV_LEFTALT
            | EV_RIGHTALT
            | EV_LEFTMETA
            | EV_RIGHTMETA
            | EV_CAPSLOCK
    )
}
