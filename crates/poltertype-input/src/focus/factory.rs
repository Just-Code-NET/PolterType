//! Per-OS constructor for the focus tracker.

use super::*;
use std::sync::Arc;

/// Build the focus tracker for the active platform. Always returns
/// *some* tracker — even on platforms where we can't read focus
/// state, we ship a noop tracker so the engine keeps a uniform API.
pub fn create_focus_tracker() -> Arc<dyn FocusTracker> {
    #[cfg(windows)]
    {
        Arc::new(windows_impl::WindowsFocusTracker)
    }
    #[cfg(not(windows))]
    {
        Arc::new(NoopFocusTracker)
    }
}
