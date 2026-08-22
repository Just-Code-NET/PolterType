//! Hyprland IPC transport + `activewindow` parsing for the focus
//! tracker.
//!
//! The socket helpers are a trimmed copy of `poltertype-layout`'s
//! Hyprland transport — those are `pub(crate)` to that crate, and a
//! shared IPC crate for ~40 lines is not worth the dependency edge.
//! Keep the two in sync if the protocol changes.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use tracing::debug;

/// Whether this machine has a Hyprland to talk to.
///
/// A live socket, not the environment variable: see
/// [`instance_signature`] for why a process can be running under
/// Hyprland with the variable unset. Twin of
/// `poltertype_layout`'s — change one, change both.
pub(crate) fn hyprland_available() -> bool {
    socket_path().is_some()
}

/// The instance signature: the environment variable when Hyprland
/// spawned us, otherwise whatever live socket directory it left.
///
/// The fallback is for a process the compositor did not spawn — an
/// autostart unit, above all. A Hyprland session imports its
/// environment into the systemd user manager from its own config, and
/// `xdg-desktop-autostart.target` can win that race, so an autostarted
/// PolterType sees a machine with no Hyprland on it at all. The socket
/// directory is there either way.
fn instance_signature() -> Option<OsString> {
    if let Some(sig) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") {
        return Some(sig);
    }
    // Newest live instance wins: a crash can leave a directory behind,
    // and a nested session is a real (if rare) thing.
    let mut newest: Option<(SystemTime, OsString)> = None;
    for dir in signature_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.path().join(".socket.sock").exists() {
                continue;
            }
            let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            if newest.as_ref().is_none_or(|(best, _)| mtime > *best) {
                newest = Some((mtime, entry.file_name()));
            }
        }
    }
    newest.map(|(_, sig)| sig)
}

/// Where Hyprland keeps its per-instance directories, newest location
/// first: `$XDG_RUNTIME_DIR/hypr/` since 0.40, `/tmp/hypr/` before it.
fn signature_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        dirs.push(PathBuf::from(xdg).join("hypr"));
    }
    dirs.push(PathBuf::from("/tmp/hypr"));
    dirs
}

/// Resolve Hyprland's request socket (`.socket.sock`). Current
/// Hyprland puts it under `$XDG_RUNTIME_DIR/hypr/<sig>/`; releases
/// before 0.40 used `/tmp/hypr/<sig>/`.
fn socket_path() -> Option<PathBuf> {
    let sig = instance_signature()?;
    signature_dirs()
        .into_iter()
        .map(|d| d.join(&sig).join(".socket.sock"))
        .find(|p| p.exists())
}

/// One request over Hyprland's IPC socket: write the command, read the
/// reply to EOF — what `hyprctl` does under the hood, minus the
/// ~20-60 ms process spawn.
fn socket_request(path: &Path, cmd: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(path)?;
    s.set_read_timeout(Some(Duration::from_millis(400)))?;
    s.set_write_timeout(Some(Duration::from_millis(400)))?;
    s.write_all(cmd.as_bytes())?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

/// The raw `activewindow` reply — socket first, `hyprctl` subprocess
/// as the fallback (covers exotic setups where the socket moved or a
/// sandbox blocks UNIX sockets but allows exec).
pub(crate) fn active_window_reply() -> Option<String> {
    if let Some(p) = socket_path() {
        match socket_request(&p, "activewindow") {
            Ok(reply) if !reply.trim_start().starts_with("unknown request") => {
                return Some(reply);
            }
            Ok(reply) => debug!(%reply, "hypr socket refused activewindow; using hyprctl"),
            Err(e) => debug!(?e, "hypr socket activewindow failed; using hyprctl"),
        }
    }
    let out = Command::new("hyprctl").arg("activewindow").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Pull `pid:` and `class:` out of the plain-text `activewindow`
/// block. Hyprland answers `Invalid` when no window is focused (empty
/// workspace, lock screen) — that parses to `(None, None)`. The
/// `initialClass:` line is deliberately NOT matched: `class:` tracks
/// what the window says about itself *now*.
pub(crate) fn parse_active_window(reply: &str) -> (Option<u32>, Option<String>) {
    let mut pid = None;
    let mut class = None;
    for line in reply.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("pid:") {
            pid = v
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|p| *p > 0)
                .and_then(|p| u32::try_from(p).ok());
        } else if let Some(v) = line.strip_prefix("class:") {
            let v = v.trim();
            if !v.is_empty() {
                class = Some(v.to_owned());
            }
        }
    }
    (pid, class)
}

/// Window rect in global logical coordinates, from the `at: x,y` and
/// `size: w,h` lines of the same `activewindow` block. `None` when
/// either is missing — half a rect would misplace the tooltip.
pub(crate) fn parse_active_window_rect(reply: &str) -> Option<(i32, i32, u32, u32)> {
    let mut at = None;
    let mut size = None;
    for line in reply.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("at:") {
            at = parse_pair(v, ',');
        } else if let Some(v) = line.strip_prefix("size:") {
            size = parse_pair(v, ',');
        }
    }
    let (x, y) = at?;
    let (w, h) = size?;
    Some((
        i32::try_from(x).ok()?,
        i32::try_from(y).ok()?,
        u32::try_from(w).ok()?,
        u32::try_from(h).ok()?,
    ))
}

/// `"11, 22"` with the given separator → `(11, 22)`.
fn parse_pair(v: &str, sep: char) -> Option<(i64, i64)> {
    let (a, b) = v.trim().split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}
