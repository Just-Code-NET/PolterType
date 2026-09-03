//! Apple keycodes and `CGEventFlags` bits `codes.rs` maps against.

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
