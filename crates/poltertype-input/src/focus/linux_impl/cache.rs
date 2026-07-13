//! TTL memoization wrapper around a real focus tracker.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::focus::FocusTracker;

/// Serves repeated `focused_exe()` calls from a short-lived cache.
/// The Linux backends each cost a UNIX-socket or X11 round-trip, and
/// two independent consumers poll: the engine at word boundaries and
/// the wordlist-profile watcher every 250 ms. Negative answers (no
/// focused window) are cached the same as positive ones.
pub(crate) struct CachedFocusTracker {
    inner: Box<dyn FocusTracker>,
    ttl: Duration,
    slot: Mutex<Option<(Instant, Option<String>)>>,
}

impl CachedFocusTracker {
    pub(crate) fn new(inner: Box<dyn FocusTracker>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            slot: Mutex::new(None),
        }
    }
}

impl FocusTracker for CachedFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        let mut slot = self.slot.lock();
        if let Some((at, value)) = slot.as_ref() {
            if at.elapsed() < self.ttl {
                return value.clone();
            }
        }
        let fresh = self.inner.focused_exe();
        *slot = Some((Instant::now(), fresh.clone()));
        fresh
    }

    fn backend_name(&self) -> &'static str {
        self.inner.backend_name()
    }
}
