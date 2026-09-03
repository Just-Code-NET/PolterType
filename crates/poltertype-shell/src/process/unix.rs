//! Unix: ask a plug-in to leave with `SIGTERM`.

/// Ask the process with this id to exit cleanly.
///
/// **Stop is a request, kill is not.** A plug-in may hold state worth
/// flushing, so it is asked first and killed only if it does not go.
/// Best-effort by nature — it may already be gone, ignore the request,
/// or be on a platform with no way to make one, and none of those are
/// errors. The caller must always have a kill.
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
