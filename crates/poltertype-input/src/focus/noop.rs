//! No-op tracker for platforms without an implementation.

use super::*;

/// Always returns `None` — used on macOS / Linux until those impls land.
pub struct NoopFocusTracker;

impl FocusTracker for NoopFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        None
    }
    fn backend_name(&self) -> &'static str {
        "noop"
    }
}
