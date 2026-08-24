//! Timing windows and fixed scancodes the engine matches against.

use std::time::Duration;

/// How long after a paste shortcut we decline to auto-correct. Covers
/// a paste replayed as a keystroke burst without swallowing the next
/// genuinely-typed word.
pub const PASTE_GUARD: Duration = Duration::from_millis(1200);

/// How long the last completed word stays reachable by the manual
/// switch-last hotkey once typing stops.
///
/// Separate from `[engine].idle_timeout_ms`, which abandons the word
/// still being typed: that one is about a caret we can no longer vouch
/// for, and two seconds is right for it. The stash is about a word
/// already on screen, and the hotkey exists precisely for the case
/// where the automatic pass did not fire — which a person needs longer
/// than two seconds to notice and act on. Everything that really
/// invalidates the stash (a click, a nav key, deleting text we never
/// saw, the next completed word) drops it through its own path; this
/// only bounds how long an untouched machine keeps one word in RAM.
pub const LAST_WORD_TTL: Duration = Duration::from_secs(60);

/// How long to wait after an emission burst before probing the key
/// stream for keystrokes that raced it — the trip from the device
/// through the listener thread into our channel. Every millisecond
/// between that probe and our next emitted key is a window for a
/// keystroke to land *inside* the correction, so the probe sits as
/// late as it can while still seeing the racer.
pub const POST_EMIT_LAG: Duration = Duration::from_millis(25);

/// Minimum gap between switching the OS layout and replaying scancodes
/// against it: the compositor propagates the new xkb state to the
/// focused client asynchronously, and a replay that outruns it comes
/// out in the layout we just left. A floor, waited out *before* the
/// deletion so it never widens the window between our last look at the
/// key stream and our first emitted key.
pub const LAYOUT_SETTLE: Duration = Duration::from_millis(30);

/// How long to give a desktop to act on its **own** switch shortcut
/// before asking whether the layout moved.
///
/// Longer than [`LAYOUT_SETTLE`] on purpose: that one waits for xkb
/// state to propagate after a switch we performed, while this waits for
/// a shell to notice a keystroke, run its handler and change the layout
/// — an event-loop round trip in another process. Measured on GNOME 49
/// and MATE, where the switch lands well inside this, 2026-08-24.
pub const CHORD_SETTLE: Duration = Duration::from_millis(180);

/// How many times a completed switch is re-read before the deletion,
/// and how long between them.
///
/// A settings daemon that disagrees with us does not disagree
/// instantly: MATE lets the group lock land and restores its own a
/// moment later, so the single reading taken 30 ms after the switch
/// said yes while the keystrokes 60 ms later still came out in the old
/// layout. Three readings across ~80 ms cover the window the deletion
/// would otherwise occupy — and cost nothing where the backend cannot
/// answer at all, which is most of them.
pub const SWITCH_HOLD_PROBES: usize = 3;
pub const SWITCH_HOLD_STEP: Duration = Duration::from_millis(40);

/// How many times a correction re-emits itself after a user keystroke
/// physically landed inside its own replay burst. Past this we stop
/// touching their text at all.
pub const INTRUSION_REPAIRS: usize = 2;

/// How many times the intrusion probe samples the key stream before
/// giving up on finding a pause, so a user who never pauses cannot
/// stall the engine.
///
/// Counted in probes rather than wall-clock: the loop's unit is one
/// [`POST_EMIT_LAG`] sleep per sample, so a clock deadline races the
/// sleeps that drive it — overshoot enough of them (142 tests on a
/// 3-core CI runner will) and the deadline expires before
/// [`INTRUSION_QUIET_PROBES`] silent samples accumulate, declining a
/// repair that should have happened.
///
/// Must stay comfortably above [`INTRUSION_QUIET_PROBES`].
pub const INTRUSION_PROBES: u8 = 24;

/// Consecutive silent probes (of [`POST_EMIT_LAG`] each) that count as
/// "the user has stopped typing" before a repair burst goes out. The
/// product must exceed a burst's own duration: a gap merely as long as
/// one inter-key interval means the next keystroke arrives mid-repair
/// and wins the same race again.
pub const INTRUSION_QUIET_PROBES: u8 = 5;

/// How long a correction keeps typing out keystrokes the key gate held
/// back, before letting the rest reach the application on its own.
/// Covers a user who carries straight on through the correction; the
/// gate's own ceiling is the hard stop.
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

/// SC Set-1 scancode for the spacebar.
///
/// A layout overlay describes the 46 character keys and nothing else,
/// so the spacebar has no entry in any of them and has to be
/// special-cased wherever held keystrokes are replayed as *text* —
/// otherwise the boundary that triggered the correction is the one
/// keystroke that never comes back.
pub const SC_SPACE: u32 = 0x39;
/// SC Set-1 scancode for Backspace. Also layout-independent, and also
/// not text — it has to be re-emitted as a keypress, in order.
pub const SC_BACKSPACE: u32 = 0x0E;
