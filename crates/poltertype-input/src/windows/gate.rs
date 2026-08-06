//! `WindowsGate` — the key gate's public face on Windows.
//!
//! Thin by design: [`HoldState`](crate::hold::HoldState) holds every
//! decision and is testable anywhere, while this type owns the two
//! things that are genuinely Windows-shaped — whether the gate is
//! switched on at all, and the clock the hook callback reads.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing::info;

use super::consts::HOLD_KEYS_ENV;
use crate::hold::HoldState;

pub struct WindowsGate {
    state: HoldState,
    enabled: AtomicBool,
}

impl Default for WindowsGate {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsGate {
    /// Build the gate, reading the opt-in once.
    ///
    /// Off unless `POLTERTYPE_HOLD_KEYS=1`. That default is the whole
    /// caution of this feature: swallowing keystrokes system-wide is
    /// the one thing in this project that can leave someone unable to
    /// type, and nobody has run this code on Windows. See
    /// `consts::HOLD_KEYS_ENV`.
    pub(crate) fn new() -> Self {
        let enabled = std::env::var(HOLD_KEYS_ENV).as_deref() == Ok("1");
        if enabled {
            info!(
                "key gate enabled by {HOLD_KEYS_ENV}=1 — keystrokes are held back during \
                 corrections. This path has not been validated on real hardware; if typing \
                 misbehaves, unset the variable and please report it (issue #7)."
            );
        }
        Self {
            state: HoldState::new(),
            enabled: AtomicBool::new(enabled),
        }
    }

    pub(crate) fn available(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Ask for the hold. Returns whether it is in force — `false` means
    /// the correction proceeds unprotected, exactly as it always has.
    pub(crate) fn hold(&self) -> bool {
        if !self.available() {
            return false;
        }
        self.state.hold();
        true
    }

    pub(crate) fn release(&self) {
        self.state.release();
    }

    /// Called from the hook callback, once per keystroke. Must stay
    /// allocation-free and lock-free.
    pub(crate) fn swallow(&self, ours: bool) -> bool {
        self.state.swallow(ours, self.state.now_ms())
    }
}
