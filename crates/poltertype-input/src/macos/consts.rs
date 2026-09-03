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

/// `kCGMouseEventButtonNumber` — which button a mouse event is about
/// (0 = left, 1 = right, 2+ = the extras).
pub(super) const K_CG_MOUSE_EVENT_BUTTON_NUMBER: u32 = 23;

/// Count of our own injected key-downs seen back by the event tap.
///
/// `CGEventPost` is fire-and-forget; the only in-process proof that the
/// window server accepted an event is its echo arriving at our own
/// session tap (stamped with [`EMITTER_TAG`]). The emitter paces a
/// backspace burst against this counter instead of a fixed sleep —
/// fields that re-query on every keystroke (Spotlight) drop deletes
/// posted on a timer, and a lost delete leaves the first letter of the
/// word standing (`ьmahou`, measured 2026-08-30).
pub(super) static INJECTED_KEYDOWN_ECHOES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Environment override for the key gate, read once at startup:
/// `POLTERTYPE_HOLD_KEYS=1` forces it on, `=0` forces it off —
/// overriding `[engine].hold_keys` in either direction. The trade is
/// latency: held keys are withheld from the application for the length
/// of the flush, which reads as the caret lagging right after a
/// correction; `docs/PERMISSIONS.md` states it.
pub(super) const HOLD_KEYS_ENV: &str = "POLTERTYPE_HOLD_KEYS";
