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

/// Pick the focus backend for this session: Hyprland first (its IPC
/// works whatever `XDG_SESSION_TYPE` says), then EWMH on plain X11.
/// Everything else — GNOME and KDE on Wayland — falls to
/// [`CaretOnlyFocusTracker`], with no TTL cache: both AT-SPI watchers
/// are event-driven and already cheap to read.
///
/// The X11 backend is deliberately **not** used on non-Hyprland Wayland
/// even when `DISPLAY` points at XWayland: XWayland sees only its own
/// windows, so `_NET_ACTIVE_WINDOW` goes stale whenever focus moves to
/// a native Wayland window — a wrong answer, worse than no answer.
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
/// compositor to ask. Like the caret watcher it owns a thread and a bus
/// connection, and failing to start is a normal, log-once condition —
/// the tracker then keeps answering `None` for `focused_exe()`.
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
