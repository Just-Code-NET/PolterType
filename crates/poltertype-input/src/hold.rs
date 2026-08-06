//! The key gate's decision, with no OS API in it.
//!
//! Everything that decides *whether to swallow a keystroke* lives here,
//! deliberately platform-free, so it compiles under `cfg(test)` on any
//! host and the safety properties get tested on machines this project
//! actually has. The Windows hook callback and the macOS event-tap
//! callback each do nothing but read an event's flags and ask
//! [`HoldState::swallow`].
//!
//! ## Why this is safer than it sounds
//!
//! A gate that swallows keystrokes system-wide is the one feature in
//! this project that can leave a user unable to type. On Linux that
//! fear is well earned: `EVIOCGRAB` outlives a wedged caller, and a
//! stuck grab took a real session down on 2026-07-31.
//!
//! Windows fails the other way, and it is worth being precise about
//! why:
//!
//! * **A dead process cannot hold the keyboard.** The hook belongs to
//!   the process; when it exits, the hook goes with it.
//! * **A hung process cannot either.** Windows removes a low-level hook
//!   whose callback overruns `LowLevelHooksTimeout`. Our callback is a
//!   couple of atomic loads, but if the process ever stopped answering,
//!   the OS itself would give the keyboard back.
//!
//! That leaves exactly one dangerous shape: a healthy, responsive
//! process that sets [`HoldState::hold`] and never clears it. The
//! deadline below is the answer, and it is checked **inside the
//! decision** rather than by a watchdog thread — so the very next
//! keystroke after expiry is the one that clears the hold and passes
//! through. No timer to miss, no thread to hang.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Ceiling on one hold, matching the evdev gate's `MAX_HOLD`. A
/// correction burst is milliseconds; this is loose enough never to cut
/// a real one short and tight enough that a bug is a hiccup rather than
/// a dead keyboard.
pub(crate) const MAX_HOLD: Duration = Duration::from_millis(1200);

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
    /// `ours` must be true for events **we** synthesised. Swallowing
    /// those would be a self-deadlock: the correction's own backspaces
    /// and letters would never reach the application it is correcting.
    /// It is not enough to ask whether an event is *injected* — another
    /// automation tool's synthetic keys are injected too, and those we
    /// do want to hold back for the same reason we hold back the
    /// user's. `listener.rs` decides `ours` from the marker the emitter
    /// stamps into `dwExtraInfo`.
    ///
    /// Expiry is handled here rather than by a watchdog: a hold past
    /// its deadline is cleared by the first event that observes it, so
    /// the worst case is one keystroke of latency and never a keyboard
    /// that stays dead.
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
