//! Names and timings shared by the evdev listener, the uinput emitter
//! and the key gate.

use std::time::Duration;

/// Name of our own `uinput` virtual keyboard. The gate must never grab
/// it (that would hold back our own corrections), and the availability
/// probe looks it up by this name.
pub(crate) const EMITTER_DEVICE_NAME: &str = "poltertype virtual keyboard";

/// Ceiling on how long the gate may hold the user's keyboard, enforced
/// by the device thread itself rather than by whoever asked for the
/// hold. A correction that hangs, panics or simply forgets to release
/// must not be able to leave the keyboard dead: the thread drops the
/// grab once this elapses, no matter what the engine is doing. Long
/// enough for a correction plus its repair passes, short enough that
/// the worst case is a hiccup rather than a lost sentence.
pub(crate) const MAX_HOLD: Duration = Duration::from_millis(1200);

/// How long `hold()` waits for the device thread to actually take the
/// grab before giving up and letting the correction proceed unheld.
/// The thread services the request on its poll cadence (~2 ms), so this
/// is many times the expected latency.
pub(crate) const HOLD_HANDSHAKE: Duration = Duration::from_millis(40);

/// How recently a keyboard must have produced an event for the gate to
/// bother holding it. Long enough to cover a pause for thought, short
/// enough that a keyboard left unplugged-but-present never costs a
/// release.
pub(crate) const RECENT_USE_WINDOW: Duration = Duration::from_secs(30);

/// How long `release()` waits for the device thread to confirm the
/// grab is gone. Bounds the blur between "held, we must type it out"
/// and "through on its own" to one poll of the read loop.
///
/// Generous on purpose: giving a device back is a 13-25 ms syscall
/// each, and the cost of giving up early is not a slow release but a
/// *wrong* one — a grab that outlives its correction makes the next
/// one count held keystrokes as though they were on screen and delete
/// text that was never there. The wait costs the user nothing; their
/// text is already on screen by this point.
pub(crate) const RELEASE_HANDSHAKE: Duration = Duration::from_millis(250);
