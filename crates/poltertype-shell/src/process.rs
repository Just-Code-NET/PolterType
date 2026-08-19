//! Spawning a plug-in's process, and asking it to stop.
//!
//! Only two operations in "supervise a plug-in" are per-platform: how a
//! child is created, which only Windows has an opinion about
//! ([`configure_child`]), and how it is asked to leave, which only Unix
//! has a mechanism for ([`request_stop`]).

use std::process::Command;

/// `CREATE_NO_WINDOW`, spelled out rather than pulled from
/// `windows-sys` — one stable ABI constant is cheaper than a Win32
/// binding crate. See `CreateProcess` → *Process Creation Flags*.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prepare a `Command` the way a tray app has to spawn a child.
///
/// PolterType links as a GUI image and owns no console, so a console
/// child spawned from it gets one **allocated**, window and all — a
/// black window beside the tray for a daemon, and one flashing up on
/// every menu draw for the state query. `CREATE_NO_WINDOW` gives the
/// child its console without a window.
#[cfg(windows)]
pub fn configure_child(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn configure_child(_cmd: &mut Command) {}

/// Ask the process with this id to exit cleanly.
///
/// **Stop is a request, kill is not.** A plug-in may hold state worth
/// flushing, so it is asked first and killed only if it does not go.
/// Best-effort by nature — it may already be gone, ignore the request,
/// or be on a platform with no way to make one, and none of those are
/// errors. The caller must always have a kill.
#[cfg(unix)]
pub fn request_stop(pid: u32) {
    // Spawning `kill` rather than linking libc keeps this crate free of
    // `unsafe` and of a C dependency, for one spawn at shutdown.
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Windows has no signal, and the console control events that stand in
/// for one cannot be sent from here: `GenerateConsoleCtrlEvent`
/// addresses a process group sharing the caller's console, and a GUI
/// image has none. Reaching one would mean `AttachConsole` onto the
/// child — process-wide state changed from the quit path of a process
/// holding a global keyboard hook.
///
/// So this is an honest no-op and the caller's kill ends the child,
/// meaning a Windows plug-in does **not** flush on the way out. See
/// `docs/DECISIONS.md`.
#[cfg(not(unix))]
pub fn request_stop(_pid: u32) {}
