//! Keys claimed for the suggestion tooltip: what the engine asked for,
//! shared with whichever listener enforces it.
//!
//! While an offer is up the engine may claim a handful of keys (the
//! arrows and Enter, or `F1`..`F9`). A listener that can keep a key
//! from the focused application does so and stamps the event
//! [`claimed`](poltertype_types::KeyEvent::claimed); one that cannot
//! never reports itself [`supported`](KeyClaims::supported), and the
//! engine then claims nothing. Plain atomics, because the readers are
//! hook callbacks that may not take a lock.
//!
//! A claim cannot outlive its deadline, so a wedged engine costs the
//! arrows a few seconds, never the session.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use crate::ClaimSet;

#[derive(Debug)]
pub(crate) struct KeyClaims {
    /// The [`ClaimSet`] bits in force.
    armed: AtomicU32,
    /// Milliseconds since `origin` past which `armed` is void.
    deadline_ms: AtomicU64,
    /// A listener that can actually keep keys back is running.
    supported: AtomicBool,
    origin: Instant,
}

impl Default for KeyClaims {
    fn default() -> Self {
        Self {
            armed: AtomicU32::new(0),
            deadline_ms: AtomicU64::new(0),
            supported: AtomicBool::new(false),
            origin: Instant::now(),
        }
    }
}

impl KeyClaims {
    /// Set by the listener that enforces claims, once it is running.
    pub(crate) fn set_supported(&self, on: bool) {
        self.supported.store(on, Ordering::Release);
        if !on {
            self.disarm();
        }
    }

    pub(crate) fn supported(&self) -> bool {
        self.supported.load(Ordering::Acquire)
    }

    /// Claim `keys` until `until`, replacing any earlier claim.
    pub(crate) fn arm(&self, keys: ClaimSet, until: Instant) {
        let ms = until.saturating_duration_since(self.origin).as_millis() as u64;
        self.deadline_ms.store(ms, Ordering::Release);
        self.armed.store(keys.bits(), Ordering::Release);
    }

    pub(crate) fn disarm(&self) {
        self.armed.store(0, Ordering::Release);
    }

    /// The claim in force right now — empty once the deadline passed.
    pub(crate) fn armed(&self) -> ClaimSet {
        if self.now_ms() >= self.deadline_ms.load(Ordering::Acquire) {
            return ClaimSet::default();
        }
        ClaimSet::from_bits(self.armed.load(Ordering::Acquire))
    }

    /// This value's clock: milliseconds since it was made.
    pub(crate) fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests;
