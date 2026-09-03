//! The key gate's decision, with no OS API in it.
//!
//! Everything that decides *whether to swallow a keystroke* lives here,
//! deliberately platform-free, so it compiles under `cfg(test)` on any
//! host. The Windows hook and macOS event-tap callbacks do nothing but
//! read an event's flags and ask [`HoldState::swallow`].
//!
//! A gate that swallows keystrokes system-wide can leave a user unable
//! to type, and on Linux that fear is earned — `EVIOCGRAB` outlives a
//! wedged caller, and a stuck grab took a real session down on
//! 2026-07-31. Windows fails the other way: the hook belongs to the
//! process, and Windows removes a low-level hook whose callback
//! overruns `LowLevelHooksTimeout`. That leaves one dangerous shape —
//! a healthy, responsive process that sets [`HoldState::hold`] and
//! never clears it — answered by the deadline below, checked *inside
//! the decision* rather than by a watchdog, so the very next keystroke
//! after expiry clears the hold and passes through.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Ceiling on one hold, matching the evdev gate's `MAX_HOLD`. A
/// correction burst is milliseconds; this is loose enough never to cut
/// a real one short and tight enough that a bug is a hiccup rather than
/// a dead keyboard.
const MAX_HOLD: Duration = Duration::from_millis(1200);

pub(crate) struct HoldState {
    /// The engine wants the user's keys held right now.
    want: AtomicBool,
    /// Milliseconds since `origin` past which the hold is void whatever
    /// `want` says.
    deadline_ms: AtomicU64,
    origin: Instant,
}

impl Default for HoldState {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldState {
    pub(crate) fn new() -> Self {
        Self {
            want: AtomicBool::new(false),
            deadline_ms: AtomicU64::new(0),
            origin: Instant::now(),
        }
    }

    pub(crate) fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }

    /// Start holding. Unlike the evdev gate there is no handshake to
    /// wait for: the decision is taken per event in the hook callback,
    /// so the store below *is* the hold taking effect.
    pub(crate) fn hold(&self) {
        self.hold_until(self.now_ms() + MAX_HOLD.as_millis() as u64);
    }

    pub(crate) fn hold_until(&self, deadline_ms: u64) {
        self.deadline_ms.store(deadline_ms, Ordering::Release);
        self.want.store(true, Ordering::Release);
    }

    pub(crate) fn release(&self) {
        self.want.store(false, Ordering::Release);
    }

    /// Only the tests ask this — the hook callback reads the decision,
    /// not the flag. Gated so a Windows build does not carry it as
    /// dead code.
    #[cfg(test)]
    pub(crate) fn is_holding(&self) -> bool {
        self.want.load(Ordering::Acquire)
    }

    /// Should this keystroke be kept from the focused application?
    ///
    /// `ours` must be true for events **we** synthesised, or the
    /// correction's own backspaces never reach the application it is
    /// correcting. Asking whether an event is merely *injected* is not
    /// enough — another automation tool's synthetic keys are injected
    /// too, and those we do want to hold back. `listener.rs` decides
    /// `ours` from the marker the emitter stamps into `dwExtraInfo`.
    ///
    /// Expiry is handled here rather than by a watchdog: a hold past
    /// its deadline is cleared by the first event that observes it, so
    /// the worst case is one keystroke of latency, never a dead
    /// keyboard.
    pub(crate) fn swallow(&self, ours: bool, now_ms: u64) -> bool {
        if ours {
            return false;
        }
        if !self.want.load(Ordering::Acquire) {
            return false;
        }
        if now_ms >= self.deadline_ms.load(Ordering::Acquire) {
            // Self-healing: whoever asked for this hold is gone or
            // wedged, and we are the last code that can undo it.
            self.want.store(false, Ordering::Release);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests;
