//! Hyprland IPC transport + `activewindow` parsing for the focus
//! tracker.
//!
//! The socket helpers are a trimmed copy of `poltertype-layout`'s
//! Hyprland transport (`poltertype-layout/src/linux/hyprland/ipc.rs`) —
//! those are `pub(crate)` to that crate, and a shared "Linux IPC" crate
//! for ~40 lines isn't worth the dependency edge yet. Keep the two in
//! sync if the socket protocol ever changes.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tracing::debug;

/// Hyprland sets `HYPRLAND_INSTANCE_SIGNATURE` on every process it
/// spawns; its presence is the activation probe, its value locates the
/// IPC socket.
pub(crate) fn hyprland_available() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

/// Resolve Hyprland's request socket (`.socket.sock`). Current
/// Hyprland puts it under `$XDG_RUNTIME_DIR/hypr/<sig>/`; releases
/// before 0.40 used `/tmp/hypr/<sig>/`.
fn socket_path() -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(xdg)
            .join("hypr")
            .join(&sig)
            .join(".socket.sock");
        if p.exists() {
            return Some(p);
        }
    }
    let legacy = PathBuf::from("/tmp/hypr").join(sig).join(".socket.sock");
    legacy.exists().then_some(legacy)
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
