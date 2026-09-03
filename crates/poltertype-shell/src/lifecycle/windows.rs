//! Windows: `taskkill` for the child, and no relaunch yet.

use std::process::Command;

/// End the Settings child at `pid`. No sweep for other windows: only
/// macOS turns a survivor into a failed update, and there is no
/// `pkill` here to make one with.
pub fn stop_ui_children(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
}

/// Not wired here yet; the caller says so and stays put.
pub fn restart_app() -> bool {
    false
}
