//! The app's own extra processes: the Settings window, and starting
//! the whole application over.
//!
//! Both are here for the same reason: a window of this executable that
//! outlives the process is not merely untidy. On macOS LaunchServices
//! then reads the app as still running, and the updater's relaunch
//! `open` reports success while only raising the orphaned old-version
//! window — the update never starts.

#[cfg(not(any(unix, windows)))]
mod other;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(not(any(unix, windows)))]
pub use other::{restart_app, stop_ui_children};
#[cfg(unix)]
pub use unix::{restart_app, stop_ui_children};
#[cfg(windows)]
pub use windows::{restart_app, stop_ui_children};
