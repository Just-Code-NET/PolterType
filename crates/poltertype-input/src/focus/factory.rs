//! Per-OS constructor for the focus tracker.

use std::sync::Arc;

#[cfg(target_os = "linux")]
use super::linux_impl::create_linux_focus_tracker as imp;
#[cfg(target_os = "macos")]
use super::macos_impl::create_macos_focus_tracker as imp;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use super::unsupported::create_unsupported_focus_tracker as imp;
#[cfg(windows)]
use super::windows_impl::create_windows_focus_tracker as imp;

use super::traits::FocusTracker;

/// Build the focus tracker for the active platform. Always returns
/// *some* tracker — even on platforms where we can't read focus
/// state, we ship a noop tracker so the engine keeps a uniform API.
pub fn create_focus_tracker() -> Arc<dyn FocusTracker> {
    imp()
}
