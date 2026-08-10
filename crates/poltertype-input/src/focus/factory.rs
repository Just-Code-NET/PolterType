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
    #[cfg(target_os = "linux")]
    {
        linux_impl::create_linux_focus_tracker()
    }
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos_impl::MacosFocusTracker)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Arc::new(NoopFocusTracker)
    }
}
