//! Apple keycodes and modifier-flag rules — plain data, no Apple API.
//!
//! Kept free of `core-graphics` so it can be exercised by `cargo test`
//! on any host: these rules are the part most likely to be wrong, since
//! a wrong scancode silently kills a word. The FFI in `listener.rs` and
//! `emitter.rs` is verified by CI's `macos-latest` job instead.

use crate::KeyDirection;

// ─── Apple virtual keycodes we care about ────────────────────────────
//
// From `<HIToolbox/Events.h>` (`kVK_*`). Layout-independent: these are
// positions, not characters.

pub(crate) const KVK_DELETE: u16 = 0x33; // "Backspace" on a PC keyboard.
pub(crate) const KVK_COMMAND: u16 = 0x37;
pub(crate) const KVK_SHIFT: u16 = 0x38;
pub(crate) const KVK_CAPS_LOCK: u16 = 0x39;
pub(crate) const KVK_OPTION: u16 = 0x3A;
pub(crate) const KVK_CONTROL: u16 = 0x3B;
pub(crate) const KVK_RIGHT_SHIFT: u16 = 0x3C;
pub(crate) const KVK_RIGHT_OPTION: u16 = 0x3D;
pub(crate) const KVK_RIGHT_CONTROL: u16 = 0x3E;
pub(crate) const KVK_RIGHT_COMMAND: u16 = 0x36;

// ─── Device-independent CGEventFlags bits ────────────────────────────
//
// Mirrors `CGEventFlags` from `IOLLEvent.h`. Spelled out rather than
// imported so this module stays host-compilable; a macOS-only test
// asserts each value against the real crate constant, so the two
// cannot drift apart unnoticed.

pub(crate) const FLAG_ALPHA_SHIFT: u64 = 0x0001_0000;
pub(crate) const FLAG_SHIFT: u64 = 0x0002_0000;
pub(crate) const FLAG_CONTROL: u64 = 0x0004_0000;
pub(crate) const FLAG_ALTERNATE: u64 = 0x0008_0000;
pub(crate) const FLAG_COMMAND: u64 = 0x0010_0000;

/// The `CGEventFlags` bit a modifier keycode owns, or `None` for keys
/// that are not modifiers *we track*.
///
/// Deliberately excludes `kVK_Function` (0x3F) and the media keys:
/// macOS reports them through the same `FlagsChanged` stream, they have
/// no SC Set-1 equivalent, and the identity fallback in
/// [`mac_keycode_to_sc1`] would land Fn inside the classifier's
/// "nav, end and discard" range — so holding Fn to reach an arrow key
/// would silently lose the word being typed.
fn modifier_flag(kvk: u16) -> Option<u64> {
    match kvk {
        KVK_SHIFT | KVK_RIGHT_SHIFT => Some(FLAG_SHIFT),
        KVK_CONTROL | KVK_RIGHT_CONTROL => Some(FLAG_CONTROL),
        KVK_OPTION | KVK_RIGHT_OPTION => Some(FLAG_ALTERNATE),
        KVK_COMMAND | KVK_RIGHT_COMMAND => Some(FLAG_COMMAND),
        KVK_CAPS_LOCK => Some(FLAG_ALPHA_SHIFT),
        _ => None,
    }
}

/// Press-or-release for a `kCGEventFlagsChanged` event.
///
/// macOS does not tell us the direction: it delivers one event type for
/// "the modifier picture changed" and expects the reader to diff it.
/// The flags describe the state *after* the change, so the bit
/// belonging to the keycode that moved is set exactly when it went
/// down.
///
/// Caps Lock is a latch: the "press" is the event turning the light on.
/// That is the shape the engine wants anyway — SC-1 0x3A classifies as
/// `Discard`, so both edges keep the word alive.
///
/// `None` means "not a modifier we mirror" — see [`modifier_flag`].
pub(crate) fn flags_changed_direction(kvk: u16, flags: u64) -> Option<KeyDirection> {
    let bit = modifier_flag(kvk)?;
    Some(if flags & bit != 0 {
        KeyDirection::Press
    } else {
        KeyDirection::Release
    })
}

// ─── Apple → Win SC Set-1 keycode mapping ────────────────────────────
//
// The engine's buffer classifier is written against Windows SC-1
// scancodes, and Apple virtual keycodes overlap that range with
// different meanings — Apple 0x39 is Caps Lock where SC-1 0x39 is
// Space. Every key the classifier pattern-matches must be translated
// explicitly; the identity fallback is safe only outside its ranges.

pub(crate) fn mac_keycode_to_sc1(kvk: u16) -> u32 {
    match kvk {
        // Letters
        0x00 => 0x1E, // A
        0x01 => 0x1F, // S
        0x02 => 0x20, // D
        0x03 => 0x21, // F
        0x04 => 0x23, // H
        0x05 => 0x22, // G
        0x06 => 0x2C, // Z
        0x07 => 0x2D, // X
        0x08 => 0x2E, // C
        0x09 => 0x2F, // V
        0x0B => 0x30, // B
        0x0C => 0x10, // Q
        0x0D => 0x11, // W
        0x0E => 0x12, // E
        0x0F => 0x13, // R
        0x10 => 0x15, // Y
        0x11 => 0x14, // T
        0x1F => 0x18, // O
        0x20 => 0x16, // U
        0x22 => 0x17, // I
        0x23 => 0x19, // P
        0x25 => 0x26, // L
        0x26 => 0x24, // J
        0x28 => 0x25, // K
        0x2D => 0x31, // N
        0x2E => 0x32, // M
        // Numbers
        0x12 => 0x02, // 1
        0x13 => 0x03, // 2
        0x14 => 0x04, // 3
        0x15 => 0x05, // 4
        0x17 => 0x06, // 5
        0x16 => 0x07, // 6
        0x1A => 0x08, // 7
        0x1C => 0x09, // 8
        0x19 => 0x0A, // 9
        0x1D => 0x0B, // 0
        // Boundaries / nav
        0x24 => 0x1C,       // Return
        0x4C => 0x1C,       // Numpad Enter
        0x30 => 0x0F,       // Tab
        0x31 => 0x39,       // Space
        KVK_DELETE => 0x0E, // Delete (= Backspace)
        0x75 => 0x53,       // Forward Delete
        0x35 => 0x01,       // Esc
        0x2B => 0x33,       // Comma
        0x2F => 0x34,       // Period
        0x2C => 0x35,       // Slash
        0x29 => 0x27,       // ;
        0x27 => 0x28,       // '
        0x21 => 0x1A,       // [
        0x1E => 0x1B,       // ]
        0x2A => 0x2B,       // backslash
        0x32 => 0x29,       // backtick
        0x18 => 0x0D,       // =
        0x1B => 0x0C,       // -
        // Modifiers, mapped onto the SC-1 slots the classifier reads as
        // "discard, stay inside the word". Unmapped, Apple 0x3C
        // (RShift) would land in the F-row range and kill any word
        // typed with it, and Apple 0x39 would alias onto SC-1 Space.
        KVK_SHIFT => 0x2A,         // LShift
        KVK_RIGHT_SHIFT => 0x36,   // RShift
        KVK_CONTROL => 0x1D,       // LControl
        KVK_RIGHT_CONTROL => 0x1D, // RControl
        KVK_OPTION => 0x38,        // LOption (Alt)
        KVK_RIGHT_OPTION => 0x38,  // ROption
        KVK_COMMAND => 0x5B,       // LCommand (SC-1 LWin)
        KVK_RIGHT_COMMAND => 0x5C, // RCommand (SC-1 RWin)
        KVK_CAPS_LOCK => 0x3A,     // Caps Lock
        // Arrow cluster (SC-1 extended positions → nav, end the word).
        0x7E => 0x48, // Up
        0x7D => 0x50, // Down
        0x7B => 0x4B, // Left
        0x7C => 0x4D, // Right
        0x73 => 0x47, // Home
        0x77 => 0x4F, // End
        0x74 => 0x49, // PageUp
        0x79 => 0x51, // PageDown
        // Function row (SC-1 F1..F10 land in the classifier's
        // end-and-discard range; F11/F12 sit outside it, same as on
        // Windows — parity, not an omission).
        0x7A => 0x3B, // F1
        0x78 => 0x3C, // F2
        0x63 => 0x3D, // F3
        0x76 => 0x3E, // F4
        0x60 => 0x3F, // F5
        0x61 => 0x40, // F6
        0x62 => 0x41, // F7
        0x64 => 0x42, // F8
        0x65 => 0x43, // F9
        0x6D => 0x44, // F10
        0x67 => 0x57, // F11
        0x6F => 0x58, // F12
        _ => kvk as u32,
    }
}
