//! Hyprland focus tracker.

use std::sync::Arc;

use crate::focus::{CaretHint, FocusTracker, FocusedWindowGeometry};

use super::atspi_caret::{AtspiCaretWatcher, CaretSample};
use super::hyprland_ipc::{
    active_window_reply, monitors_reply, parse_active_window, parse_active_window_rect,
    parse_monitors,
};
use super::proc_exe::exe_basename_for_pid;

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
        let (x, y, width, height, monitor_id) = parse_active_window_rect(&reply)?;
        // Resolve the owning monitor for output-local placement.
        // A miss (hotplug race, parse drift across Hyprland versions)
        // degrades to "no output info" — the popup then lets the
        // compositor pick the output, which is still on screen.
        let monitor = monitors_reply()
            .map(|r| parse_monitors(&r))
            .and_then(|ms| ms.into_iter().find(|m| m.id == monitor_id));
        let (output, output_x, output_y) = match monitor {
            Some(m) => (Some(m.name), m.x, m.y),
            None => (None, 0, 0),
        };
        Some(FocusedWindowGeometry {
            x,
            y,
            width,
            height,
            output,
            output_x,
            output_y,
        })
    }

    fn caret_hint(&self) -> Option<CaretHint> {
        self.caret.as_ref()?.latest().map(CaretSample::into_hint)
    }

    fn backend_name(&self) -> &'static str {
        "linux-hyprland-ipc"
    }
}
