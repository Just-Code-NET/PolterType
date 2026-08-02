//! Backend probe for the Linux focus tracker.

use std::sync::Arc;

use tracing::info;

use crate::focus::{FocusTracker, NoopFocusTracker};
use crate::linux::{SessionKind, session_kind};

use super::atspi_caret::AtspiCaretWatcher;
use super::atspi_focus::AtspiFocusWatcher;
use super::cache::CachedFocusTracker;
use super::caret_only::CaretOnlyFocusTracker;
use super::consts::FOCUS_CACHE_TTL;
use super::hyprland::HyprlandFocusTracker;
use super::hyprland_ipc::hyprland_available;
use super::x11::X11FocusTracker;

/// Pick the focus backend for this session. Hyprland is probed first
/// (its IPC works regardless of what `XDG_SESSION_TYPE` says), then
/// plain X11 sessions get EWMH. Everything else — GNOME / KDE on
/// Wayland — has no compositor-agnostic active-window query, by
/// design.
///
/// It does **not** follow that those sessions get nothing. AT-SPI is a
/// session-bus service and does not care which compositor is running,
/// so it answers on GNOME and KDE exactly as it does on Hyprland, and
/// it supplies *both* halves:
///
/// * the caret, which is the tooltip's best anchor — better than the
///   window geometry the other backends can offer;
/// * the focused application, by asking the a11y bus which process
///   owns the connection that sent a `window:activate`.
///
/// The second one is why `focused_exe()` is no longer flatly `None`
/// on GNOME and KDE. Read [`super::atspi_focus`] before relying on
/// it: an application with no accessibility bridge — most terminals —
/// is invisible to it, so it is an improvement on nothing rather than
/// an equivalent of a compositor answer.
///
/// No TTL cache on this branch: both watchers are event-driven and
/// already cheap to read.
///
/// Note the X11 backend is deliberately NOT used on non-Hyprland
/// Wayland even when `DISPLAY` points at XWayland: XWayland only sees
/// its own windows, so its `_NET_ACTIVE_WINDOW` would go stale every
/// time focus moves to a native Wayland window — a *wrong* answer,
/// which is worse than no answer.
pub(crate) fn create_linux_focus_tracker() -> Arc<dyn FocusTracker> {
    if hyprland_available() {
        return Arc::new(CachedFocusTracker::new(
            Box::new(HyprlandFocusTracker::new(caret_watcher())),
            FOCUS_CACHE_TTL,
        ));
    }
    if session_kind() == SessionKind::X11 {
        return Arc::new(CachedFocusTracker::new(
            Box::new(X11FocusTracker::new(caret_watcher())),
            FOCUS_CACHE_TTL,
        ));
    }
    match caret_watcher() {
        Some(caret) => Arc::new(CaretOnlyFocusTracker::new(caret, focus_watcher())),
        None => Arc::new(NoopFocusTracker),
    }
}

/// The AT-SPI focused-application watcher, for the branch that has no
/// compositor to ask. Like the caret watcher it owns a thread and a
/// bus connection, and failing to start is a normal, log-once
/// condition rather than an error — the tracker simply keeps
/// answering `None` for `focused_exe()`, which is where these
/// sessions were before.
fn focus_watcher() -> Option<Arc<AtspiFocusWatcher>> {
    match AtspiFocusWatcher::try_new() {
        Ok(w) => Some(Arc::new(w)),
        Err(e) => {
            info!(%e, "AT-SPI focus watcher unavailable; per-app features stay inert");
            None
        }
    }
}

/// One AT-SPI caret watcher per tracker — created only for a branch
/// that actually builds one (the probe branches are exclusive, so
/// this runs at most once per factory call). It owns a thread and a
/// bus connection, hence the sharing via `Arc` rather than a
/// per-backend instance. Failure is a normal, log-once condition:
/// headless CI, a11y stack disabled or absent.
fn caret_watcher() -> Option<Arc<AtspiCaretWatcher>> {
    match AtspiCaretWatcher::try_new() {
        Ok(w) => Some(Arc::new(w)),
        Err(e) => {
            info!(%e, "AT-SPI caret watcher unavailable; tooltip anchoring falls back to the window");
            None
        }
    }
}
