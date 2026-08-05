//! `MacosGate` — the key gate's public face on macOS.
//!
//! Thin by design, mirroring the Windows gate: the swallow decision
//! lives in [`HoldState`](crate::hold::HoldState) (pure, tested
//! everywhere); this type owns the two things that are genuinely
//! macOS-shaped — whether the gate is on at all, and whether the event
//! tap is actually there to do the swallowing.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing::{debug, info};

use crate::hold::HoldState;

/// Environment override for the key gate, read once at startup.
///
/// `POLTERTYPE_HOLD_KEYS=0` turns the gate off; anything else (or
/// unset) leaves it on. The **default on macOS is on**: the tap
/// callback's swallow decision is a couple of atomic loads, a past-
/// deadline hold clears itself on the next keystroke, and a dead or
/// wedged process cannot hold the keyboard — the tap dies with the
/// process, and macOS disables one whose callback stops answering
/// (which we survive by re-enabling on the disabled event itself).
/// Same override name as Windows, whose default is the opposite
/// (`windows/consts.rs` explains why theirs is opt-in).
pub(crate) const HOLD_KEYS_ENV: &str = "POLTERTYPE_HOLD_KEYS";

pub struct MacosGate {
    state: HoldState,
    /// The env override, read once.
    enabled: bool,
    /// The tap thread attached its tap and is servicing it. The engine
    /// must never believe keys are held when nothing is listening —
    /// with no tap, `swallow` never fires and the user's keystrokes
    /// reach applications as always, so reporting `available` then
    /// would make a correction skip its compensation path and lose
    /// text.
    tap_running: AtomicBool,
}

impl Default for MacosGate {
    fn default() -> Self {
        Self::new()
    }
}

impl MacosGate {
    pub(crate) fn new() -> Self {
        let enabled = std::env::var(HOLD_KEYS_ENV).as_deref() != Ok("0");
        if !enabled {
            info!("key gate disabled by {HOLD_KEYS_ENV}=0");
        }
        Self {
            state: HoldState::new(),
            enabled,
            tap_running: AtomicBool::new(false),
        }
    }

    pub(crate) fn available(&self) -> bool {
        self.enabled && self.tap_running.load(Ordering::Acquire)
    }

    /// Whether the tap should be created active (able to swallow) —
    /// i.e. the gate is administratively on. The tap decides this at
    /// creation; runtime availability additionally needs the tap up.
    pub(crate) fn wants_active_tap(&self) -> bool {
        self.enabled
    }

    /// Ask for the hold. Returns whether it is in force — `false` means
    /// the correction proceeds unprotected, exactly as it always has.
    pub(crate) fn hold(&self) -> bool {
        if !self.available() {
            return false;
        }
        self.state.hold();
        debug!("key gate: holding");
        true
    }

    pub(crate) fn release(&self) {
        self.state.release();
        debug!("key gate: released");
    }

    /// Called from the tap callback, once per keystroke. Must stay
    /// allocation-free and lock-free — a callback that blocks gets the
    /// tap disabled by the OS.
    pub(crate) fn swallow(&self, ours: bool) -> bool {
        let s = self.state.swallow(ours, self.state.now_ms());
        if s {
            debug!("key gate: swallowing user keystroke");
        }
        s
    }

    /// The tap thread reports its lifecycle here.
    pub(crate) fn set_tap_running(&self, running: bool) {
        self.tap_running.store(running, Ordering::Release);
        if running {
            debug!("key gate: tap running — holds are possible");
        } else {
            // The tap is gone; nothing can swallow now. Clear any
            // armed hold so the next correction doesn't think keys
            // are held when they are reaching applications.
            self.state.release();
            debug!("key gate: tap stopped — holds unavailable");
        }
    }
}
