//! Tunables for the Linux focus tracker.

use std::time::Duration;

/// How long a focus answer may be served from cache. The wordlist-
/// profile watcher asks every 250 ms and the engine asks on every word
/// boundary; one real IPC/X11 round-trip per 150 ms window keeps all
/// of them cheap while staying far below human app-switching cadence.
pub(crate) const FOCUS_CACHE_TTL: Duration = Duration::from_millis(150);
