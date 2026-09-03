//! Unix: signals for the child, and `open` for the bundle.

use std::path::Path;
use std::process::Command;

/// End the Settings child at `pid` — 0 when none is tracked — and every
/// other window this executable still has open.
///
/// The sweep is not belt-and-braces. There is no second-window guard,
/// so a Settings window and a Setup-alert window can coexist while the
/// one pid slot remembers only the later of them; whichever survives
/// the main process is what makes LaunchServices treat the app as
/// still running (measured with `poltertype --setup`, 2026-08-31).
pub fn stop_ui_children(pid: u32) {
    if pid != 0 {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new("pkill")
            .arg("-f")
            .arg(format!("{} --", exe.display()))
            .status();
    }
}

/// Stop the process that owns this one and start the application
/// again, returning whether this platform has a way to.
///
/// Called from the Settings window, which is a child of the tray
/// process. The relaunch is detached and delayed on purpose: while
/// this process is alive LaunchServices reads the app as already
/// running and `open` merely activates this window, which is the ghost
/// that used to eat the updater's relaunch. **The caller must exit
/// promptly** — the `open` lands after we are gone.
///
/// Outside an `.app` bundle there is nothing to reopen and this is a
/// plain quit.
pub fn restart_app() -> bool {
    let parent = std::os::unix::process::parent_id();
    if parent > 1 {
        let _ = Command::new("kill").arg(parent.to_string()).status();
    }
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent()?.parent().map(Path::to_path_buf));
    if let Some(bundle) = bundle {
        if bundle.extension().is_some_and(|x| x == "app") {
            let _ = Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "sleep 2; open '{}'",
                    bundle.display().to_string().replace('\'', "'\\''")
                ))
                .spawn();
        }
    }
    true
}
