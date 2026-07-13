//! Backend probe for the Linux focus tracker.

use std::sync::Arc;

use crate::focus::{FocusTracker, NoopFocusTracker};
use crate::linux::{SessionKind, session_kind};

use super::cache::CachedFocusTracker;
use super::consts::FOCUS_CACHE_TTL;
use super::hyprland::HyprlandFocusTracker;
use super::hyprland_ipc::hyprland_available;
use super::x11::X11FocusTracker;

/// Pick the focus backend for this session. Hyprland is probed first
/// (its IPC works regardless of what `XDG_SESSION_TYPE` says), then
/// plain X11 sessions get EWMH. Everything else — GNOME / KDE on
/// Wayland — stays on the noop tracker: there is no compositor-
/// agnostic active-window query there, by design.
///
/// Note the X11 backend is deliberately NOT used on non-Hyprland
/// Wayland even when `DISPLAY` points at XWayland: XWayland only sees
/// its own windows, so its `_NET_ACTIVE_WINDOW` would go stale every
/// time focus moves to a native Wayland window — a *wrong* answer,
/// which is worse than no answer.
pub(crate) fn create_linux_focus_tracker() -> Arc<dyn FocusTracker> {
    if hyprland_available() {
        return Arc::new(CachedFocusTracker::new(
            Box::new(HyprlandFocusTracker),
            FOCUS_CACHE_TTL,
        ));
    }
    if session_kind() == SessionKind::X11 {
        return Arc::new(CachedFocusTracker::new(
            Box::new(X11FocusTracker::new()),
            FOCUS_CACHE_TTL,
        ));
    }
    Arc::new(NoopFocusTracker)
}
