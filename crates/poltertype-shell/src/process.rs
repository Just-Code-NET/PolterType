//! Spawning a plug-in's process, and asking it to stop.
//!
//! Lives here rather than in the app because these are the operations
//! in "supervise a plug-in" whose meaning is per-platform, and this
//! crate is where per-OS app-shell quirks belong. Everything else the
//! app does with plug-in processes — arguments, reaping, killing — is
//! `std::process` and needs no `cfg` at all.
//!
//! Two unrelated divergences, one file:
//!
//! * [`configure_child`] — how a child is created, which only Windows
//!   has an opinion about;
//! * [`request_stop`] — how it is asked to leave, which only Unix has a
//!   mechanism for.

use std::process::Command;

/// `CREATE_NO_WINDOW`. Run a console program without giving it a
/// console window of its own.
///
/// Spelled out rather than pulled from `windows-sys`: it is one stable
/// ABI constant, and a dependency on a Win32 binding crate for a single
/// `u32` would be the larger cost. Documented under
/// `CreateProcess` → *Process Creation Flags*.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prepare a `Command` the way a tray app has to spawn a child.
///
/// PolterType links as a GUI image (`windows_subsystem = "windows"` in
/// `poltertype-app`), so it owns no console. A console child spawned
/// from it therefore gets one **allocated**, window and all — which for
/// a plug-in daemon means a black window appearing beside the tray, and
/// for the state query behind the tray menu means one flashing up every
/// single time the menu is drawn. `CREATE_NO_WINDOW` gives the child
/// its console without a window to show for it.
///
/// Nothing to configure elsewhere: no other platform we ship attaches a
/// window to a process for being a console program.
#[cfg(windows)]
pub fn configure_child(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn configure_child(_cmd: &mut Command) {}

/// Ask the process with this id to exit cleanly.
///
/// Best-effort by nature: the process may already be gone, may ignore
/// the request, or may be on a platform with no way to make it. None
/// of those are errors here — the caller's timeout and kill are what
/// make stopping certain.
///
/// The distinction that matters: **stop is a request, kill is not.** A
/// plug-in may hold state worth flushing on the way out (the first one
/// written against this interface has an in-flight buffer it would
/// otherwise lose), so it is asked first and killed only if it does
/// not go. Where a platform has no way to ask, this is honestly a
/// no-op and the caller's kill is what ends it — which is why the
/// caller must always have one.
#[cfg(unix)]
pub fn request_stop(pid: u32) {
    // Spawning `kill` rather than linking libc: this crate has no
    // unsafe and no C dependency, and one process spawn at shutdown is
    // not worth either. `kill` is in POSIX, so it is present wherever
    // this arm compiles.
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Windows has no signal, and the console control events that stand in
/// for one cannot be sent from here: `GenerateConsoleCtrlEvent`
/// addresses a process **group sharing the caller's console**, and a
/// GUI image has no console to share. Reaching one would mean
/// `AttachConsole` onto the child, which is process-wide state changed
/// from the quit path of a process holding a global keyboard hook.
///
/// So this stays an honest no-op and the caller's kill is what ends the
/// child — meaning a Windows plug-in does **not** get to flush on the
/// way out. Documented rather than papered over; see `docs/DECISIONS.md`.
#[cfg(not(unix))]
pub fn request_stop(_pid: u32) {}
