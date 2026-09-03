//! Hyprland focus tracker.

use std::sync::Arc;

use crate::focus::{CaretHint, FocusTracker, FocusedWindowGeometry};

use super::atspi_caret::AtspiCaretWatcher;
use super::hyprland_ipc::{active_window_reply, parse_active_window, parse_active_window_rect};
use super::proc_exe::exe_basename_for_pid;
use super::types::CaretSample;

/// Focus via Hyprland's `activewindow` IPC query. Prefers the window's
/// PID resolved through `/proc` (the exact analogue of the Windows
/// tracker's process-image basename); falls back to the window class
/// when `/proc` is unreadable (sandboxed apps).
pub(crate) struct HyprlandFocusTracker {
    /// Shared AT-SPI caret watcher; `None` when the a11y bus is
    /// unavailable (the tooltip then anchors to the window).
    caret: Option<Arc<AtspiCaretWatcher>>,
}

impl HyprlandFocusTracker {
    pub(crate) fn new(caret: Option<Arc<AtspiCaretWatcher>>) -> Self {
        Self { caret }
    }
}

impl FocusTracker for HyprlandFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        let reply = active_window_reply()?;
        let (pid, class) = parse_active_window(&reply);
        pid.and_then(exe_basename_for_pid).or(class)
    }

    fn focused_window_geometry(&self) -> Option<FocusedWindowGeometry> {
        let reply = active_window_reply()?;
        let (x, y, width, height) = parse_active_window_rect(&reply)?;
        let (pid, _) = parse_active_window(&reply);
        Some(FocusedWindowGeometry {
            x,
            y,
            width,
            height,
            pid,
        })
    }

    fn caret_hint(&self) -> Option<CaretHint> {
        self.caret.as_ref()?.latest().map(CaretSample::into_hint)
    }

    fn backend_name(&self) -> &'static str {
        "linux-hyprland-ipc"
    }
}
