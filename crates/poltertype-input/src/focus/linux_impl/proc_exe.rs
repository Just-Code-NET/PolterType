//! PID → executable-basename resolution via `/proc`.

/// Basename of the executable behind `pid`, e.g. `"alacritty"`.
///
/// `/proc/<pid>/exe` is the truthful answer — a symlink to the real
/// binary, unaffected by argv\[0\] games — and is readable only for
/// same-UID processes, exactly the set that owns the user's windows. A
/// failure falls back to `/proc/<pid>/comm`, world-readable but
/// truncated to 15 bytes. Either is enough for the `disabled_apps`
/// basename match.
pub(crate) fn exe_basename_for_pid(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    if exe.is_some() {
        return exe;
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
