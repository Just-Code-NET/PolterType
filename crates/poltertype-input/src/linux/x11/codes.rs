//! Keycode / keysym translation. Pure functions — unit-tested in
//! `tests.rs` without an X server.

use super::consts::*;

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

/// Is this XInput2 button number a caret-moving click?
///
/// Buttons 1/2/3 are left/middle/right and 8/9 are back/forward — all
/// of them can move the text cursor, which the engine must know about
/// or its word buffer silently diverges from what's on screen. Buttons
/// 4–7 are the scroll wheel's four directions: scrolling doesn't move
/// the caret, so reporting them would abandon the word buffer for no
/// reason. (The evdev backend gets this for free — the kernel reports
/// scroll as `REL_WHEEL`, not a button.)
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
