//! The per-keystroke claim decision of the hooks that drop events
//! themselves — the Windows low-level hook and the macOS event tap.
//! No OS API in it, so it compiles and is tested on every host, like
//! `hold.rs`.
//!
//! A key is claimed whole — press, autorepeat and release — or not at
//! all: an application that sees only the release of a key, or only its
//! repeats, acts on a keystroke nobody meant for it. Hence the `down`
//! set, which outlives the claim that started it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::claims::KeyClaims;
use crate::{ClaimKey, KeyDirection, KeyEvent, Modifiers};

/// How long a kept-back press vouches for the rest of its key's
/// keystrokes without hearing from it. Autorepeat refreshes it many
/// times a second, so only a release that never arrived — a listener
/// restart, a lost event — runs it out, and then the key comes back
/// instead of staying dead.
const DOWN_STALE_MS: u64 = 2000;

pub(crate) struct ClaimFilter {
    claims: Arc<KeyClaims>,
    /// Keys whose press was kept back and whose release has not come
    /// yet.
    down: AtomicU32,
    /// When a kept-back press was last seen, on `claims`' clock.
    down_at_ms: AtomicU64,
}

impl ClaimFilter {
    pub(crate) fn new(claims: Arc<KeyClaims>) -> Self {
        Self {
            claims,
            down: AtomicU32::new(0),
            down_at_ms: AtomicU64::new(0),
        }
    }

    /// Should this keystroke be kept from the focused application?
    ///
    /// `key` is the event's claimable identity, if it has one, and
    /// `mods` the modifiers held with it: exactly the accept chord's for
    /// a digit, none for anything else — `Shift+Down` extends a
    /// selection and is never ours. `ours` must be true for events we
    /// synthesised. Allocation-free and lock-free: the hooks call it
    /// once per keystroke.
    pub(crate) fn decide(
        &self,
        key: Option<ClaimKey>,
        press: bool,
        mods: Modifiers,
        ours: bool,
    ) -> bool {
        self.decide_at(key, press, mods, ours, self.claims.now_ms())
    }

    /// [`decide`](Self::decide) for a translated event, whose claimable
    /// identity the hook worked out from its own key code. The event's
    /// `injected` must mean "ours", as both hooks' does.
    pub(crate) fn decide_event(&self, key: Option<ClaimKey>, ev: &KeyEvent) -> bool {
        self.decide(
            key,
            ev.direction == KeyDirection::Press,
            ev.modifiers,
            ev.injected,
        )
    }

    fn decide_at(
        &self,
        key: Option<ClaimKey>,
        press: bool,
        mods: Modifiers,
        ours: bool,
        now: u64,
    ) -> bool {
        let Some(key) = key else {
            return false;
        };
        if ours {
            return false;
        }
        let bit = key.bit();
        if now.saturating_sub(self.down_at_ms.load(Ordering::Acquire)) > DOWN_STALE_MS {
            self.down.store(0, Ordering::Release);
        }
        if self.down.load(Ordering::Acquire) & bit != 0 {
            if press {
                self.down_at_ms.store(now, Ordering::Release);
            } else {
                self.down.fetch_and(!bit, Ordering::AcqRel);
            }
            return true;
        }
        if !press {
            return false;
        }
        let armed = self.claims.armed();
        let want = match key {
            ClaimKey::Digit(_) => armed.digit_modifiers(),
            _ => Modifiers::NONE,
        };
        let held_as_wanted = mods.control == want.control
            && mods.shift == want.shift
            && mods.alt == want.alt
            && mods.meta == want.meta;
        if !held_as_wanted || !armed.contains(key) {
            return false;
        }
        self.down_at_ms.store(now, Ordering::Release);
        self.down.fetch_or(bit, Ordering::AcqRel);
        true
    }
}

#[cfg(test)]
mod tests;
