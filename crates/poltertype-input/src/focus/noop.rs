//! No-op tracker for platforms without an implementation.

use super::*;

/// Always returns `None` — the arm for a target with no tracker at
/// all. Windows, macOS and Linux each have one; this is what a fourth
/// platform would compile against.
pub struct NoopFocusTracker;

impl FocusTracker for NoopFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        None
    }
    fn backend_name(&self) -> &'static str {
        "noop"
    }
}
