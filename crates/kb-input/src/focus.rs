//! Per-OS foreground-app tracking.
//!
//! The engine consults this to decide whether auto-switching is
//! appropriate for the focused application — the dev-friendly path
//! that keeps the corrector silent in IDEs / terminals while you're
//! typing code (see `docs/DECISIONS.md`).
//!
//! The trait is intentionally minimal: just "what's the executable
//! name of the focused window?". That's enough to match against the
//! `[exceptions].disabled_apps` list. Window class / title matching
//! land in v0.1.x if needed.

#![deny(unsafe_op_in_unsafe_fn)]

use std::sync::Arc;

#[cfg(windows)]
mod windows_impl;

/// Best-effort identifier of the currently-focused application.
pub trait FocusTracker: Send + Sync {
    /// File-name of the focused process's executable, e.g.
    /// `"Code.exe"` / `"alacritty"`. Returns `None` if no foreground
    /// window exists, the OS denies the query, or this platform's
    /// implementation is a stub.
    fn focused_exe(&self) -> Option<String>;

    fn backend_name(&self) -> &'static str;
}

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
