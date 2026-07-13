//! Hyprland focus tracker.

use crate::focus::FocusTracker;

use super::hyprland_ipc::{active_window_reply, parse_active_window};
use super::proc_exe::exe_basename_for_pid;

/// Focus via Hyprland's `activewindow` IPC query. Prefers the window's
/// PID resolved through `/proc` (the exact analogue of the Windows
/// tracker's process-image basename); falls back to the window class
/// when `/proc` is unreadable (sandboxed apps).
pub(crate) struct HyprlandFocusTracker;

impl FocusTracker for HyprlandFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        let reply = active_window_reply()?;
        let (pid, class) = parse_active_window(&reply);
        pid.and_then(exe_basename_for_pid).or(class)
    }

    fn backend_name(&self) -> &'static str {
        "linux-hyprland-ipc"
    }
}
