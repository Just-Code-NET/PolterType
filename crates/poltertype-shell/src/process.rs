//! Asking another process to stop.
//!
//! Lives here rather than in the app because it is the one operation
//! in "supervise a plug-in" whose meaning is per-platform, and this
//! crate is where per-OS app-shell quirks belong. Everything the app
//! itself does with plug-in processes — spawning, reaping, killing —
//! is `std::process` and needs no `cfg` at all.
//!
//! The distinction that matters: **stop is a request, kill is not.** A
//! plug-in may hold state worth flushing on the way out (the first one
//! written against this interface has an in-flight buffer it would
//! otherwise lose), so it is asked first and killed only if it does
//! not go. Where a platform has no way to ask, this is honestly a
//! no-op and the caller's kill is what ends it — which is why the
//! caller must always have one.

/// Ask the process with this id to exit cleanly.
///
/// Best-effort by nature: the process may already be gone, may ignore
/// the request, or may be on a platform with no way to make it. None
/// of those are errors here — the caller's timeout and kill are what
/// make stopping certain.
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

/// Windows has no signal a console-less process can act on, so there
/// is nothing to ask. The caller kills after its grace period.
#[cfg(not(unix))]
pub fn request_stop(_pid: u32) {}
