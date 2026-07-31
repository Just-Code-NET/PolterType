//! `CGEvent` field ids and the tag we stamp on our own emissions.

/// `kCGKeyboardEventKeycode`.
///
/// `CGEventField` is a `u32` enum-like in Apple's C header; the
/// `core-graphics` crate has represented it differently across
/// releases, so we hard-code the documented integer values rather than
/// depend on whichever variant naming the active version exposes.
pub(super) const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;

/// `kCGEventSourceUserData`.
pub(super) const K_CG_EVENT_SOURCE_USER_DATA: u32 = 42;

/// Magic value stamped into `kCGEventSourceUserData` on every event
/// WE post, so the listener can tag them `injected` and the engine
/// never mistakes our own backspaces / retypes for user keystrokes.
/// Without this the emitted events echo back through the tap as
/// "real" input: the backspace burst poisons the word buffer right
/// after a correction, and every second word gets skipped as tainted.
pub(super) const EMITTER_TAG: i64 = 0x504F_4C54; // "POLT"
