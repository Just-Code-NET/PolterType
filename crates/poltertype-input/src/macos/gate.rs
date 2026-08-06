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
/// `POLTERTYPE_HOLD_KEYS=1` turns it on, `=0` off. The **default on
/// macOS is off — same as Windows, and for the same current reason:
/// not fear, but latency.** Held keys are withheld from the
/// application for the length of the flush (engine-side:
/// `HELD_FLUSH_QUIET_PROBES × POST_EMIT_LAG`, ceiling `HELD_FLUSH`),
/// which reads as the caret lagging behind your typing after every
/// correction. Switch it on if you type fast enough to hit the race;
/// `docs/PERMISSIONS.md` states the trade. See
/// `windows/consts::HOLD_KEYS_ENV`.
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
        let enabled = std::env::var(HOLD_KEYS_ENV).as_deref() == Ok("1");
        if enabled {
            info!(
                "key gate enabled by {HOLD_KEYS_ENV}=1 — keystrokes are held back during \
                 corrections, at a small delay after each one (see docs/PERMISSIONS.md)"
            );
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn unavailable_until_the_tap_reports_running() {
        // Enabled via env so the test is independent of the default.
        unsafe { std::env::set_var(HOLD_KEYS_ENV, "1") };
        let g = MacosGate::new();
        assert!(!g.available(), "no tap yet — must not claim to hold");
        assert!(!g.hold(), "hold without a tap reports unheld");
        g.set_tap_running(true);
        assert!(g.available());
        assert!(g.hold());
        g.set_tap_running(false);
        assert!(!g.available(), "tap gone — holds unavailable again");
        unsafe { std::env::remove_var(HOLD_KEYS_ENV) };
    }

    #[test]
    fn env_zero_disables_even_with_a_running_tap() {
        unsafe { std::env::set_var(HOLD_KEYS_ENV, "0") };
        let g = MacosGate::new();
        g.set_tap_running(true);
        assert!(!g.available());
        assert!(!g.hold());
        unsafe { std::env::remove_var(HOLD_KEYS_ENV) };
    }

    #[test]
    fn default_is_opt_in() {
        unsafe { std::env::remove_var(HOLD_KEYS_ENV) };
        let g = MacosGate::new();
        g.set_tap_running(true);
        assert!(
            !g.available(),
            "default must be opt-in (latency trade — see docs/PERMISSIONS.md)"
        );
    }
}
