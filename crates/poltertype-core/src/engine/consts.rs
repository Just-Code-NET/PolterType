//! Timing windows and fixed scancodes the engine matches against.

use std::time::Duration;

/// How long after a paste shortcut we decline to auto-correct. Generous
/// enough to cover a paste replayed as a keystroke burst, short enough
/// that the next genuinely-typed word still gets corrected.
pub const PASTE_GUARD: Duration = Duration::from_millis(1200);

/// How long to wait after an emission burst before probing the key
/// stream for keystrokes that raced it: the trip from the device
/// through the listener thread into our channel. Every millisecond
/// between that probe and our next emitted key is a window in which a
/// physical keystroke lands on screen *inside* the correction, so the
/// probe sits as late as possible — and this is what makes it late
/// enough to see.
pub const POST_EMIT_LAG: Duration = Duration::from_millis(25);

/// Minimum gap between switching the OS layout and replaying scancodes
/// against it — the compositor propagates the new xkb state to the
/// focused client asynchronously, and a replay that outruns it comes
/// out in the layout we just left. The absorb gate plus the backspace
/// burst normally cover this many times over; it is a floor, waited
/// out *before* the deletion so it never widens the window between our
/// last look at the key stream and our first emitted key.
pub const LAYOUT_SETTLE: Duration = Duration::from_millis(30);

/// How many times a correction re-emits itself after finding that a
/// user keystroke physically landed inside its own replay burst. Two
/// is enough for a fast typist to lose the race twice and still end up
/// with correct text; past that we stop touching their text at all.
pub const INTRUSION_REPAIRS: usize = 2;

/// How many times the intrusion probe samples the key stream before
/// giving up on finding a pause. A repair is itself a burst, so
/// starting one while the user is still mid-word only moves the
/// scramble along — better to leave the text alone and stop vouching
/// for the screen. Bounded so a user who never pauses can't stall the
/// engine.
///
/// Counted in probes rather than wall-clock on purpose. The loop's
/// own unit is one [`POST_EMIT_LAG`] sleep per sample, so a deadline
/// measured by the clock races the sleeps that drive it: overshoot
/// enough of them — 142 tests on a 3-core CI runner will — and the
/// deadline expires before [`INTRUSION_QUIET_PROBES`] silent samples
/// can accumulate, and the engine declines a repair it should have
/// made. That surfaced as an intermittent test failure on macOS CI,
/// but the same load makes a real user lose a correction. Counting
/// samples keeps the decision identical whatever the scheduler does;
/// only the wall-clock duration stretches, which is exactly the part
/// that should stretch on a busy machine.
///
/// Must stay comfortably above [`INTRUSION_QUIET_PROBES`], the
/// shortest run of silence that authorises a repair.
pub const INTRUSION_PROBES: u8 = 24;

/// Consecutive silent probes (of [`POST_EMIT_LAG`] each) that count as
/// "the user has stopped typing" before a repair burst goes out. The
/// product must exceed a burst's own duration — a gap merely as long
/// as one inter-key interval means the next keystroke arrives mid-
/// repair and wins the same race again, which is how a repair turns
/// into a correction chasing the user's fingers down the line.
pub const INTRUSION_QUIET_PROBES: u8 = 5;

/// How long a correction keeps typing out keystrokes the key gate held
/// back, before letting go and leaving the rest to reach the
/// application on its own. Covers a user who carries straight on
/// through the correction without stretching the hold — the gate's own
/// ceiling is the hard stop.
pub const HELD_FLUSH: Duration = Duration::from_millis(250);

/// Consecutive empty sweeps (of [`POST_EMIT_LAG`] each) that end the
/// flush. One is not enough: it is shorter than an inter-key gap, so
/// letting go on it drops whatever the user presses in the moment
/// between our last sweep and the grab actually lifting.
pub const HELD_FLUSH_QUIET_PROBES: u8 = 3;

/// SC Set-1 scancode for the `V` key (matches evdev `KEY_V` on Linux).
pub const SC_V: u32 = 0x2F;
/// evdev `KEY_INSERT` — used for the Shift+Insert paste shortcut. (Insert
/// has no plain SC-1 byte; the listener reports the raw evdev code.)
pub const SC_INSERT: u32 = 110;
