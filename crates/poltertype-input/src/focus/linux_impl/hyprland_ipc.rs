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

/// Window rect (global logical coordinates) + owning monitor id from
/// the same `activewindow` block: the `at: x,y`, `size: w,h` and
/// `monitor: N` lines. `None` when any of the three is missing —
/// a partial answer would misplace the tooltip.
pub(crate) fn parse_active_window_rect(reply: &str) -> Option<(i32, i32, u32, u32, i64)> {
    let mut at = None;
    let mut size = None;
    let mut monitor = None;
    for line in reply.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("at:") {
            at = parse_pair(v, ',');
        } else if let Some(v) = line.strip_prefix("size:") {
            size = parse_pair(v, ',');
        } else if let Some(v) = line.strip_prefix("monitor:") {
            monitor = v.trim().parse::<i64>().ok();
        }
    }
    let (x, y) = at?;
    let (w, h) = size?;
    Some((
        x as i32,
        y as i32,
        u32::try_from(w).ok()?,
        u32::try_from(h).ok()?,
        monitor?,
    ))
}

/// One entry of the `monitors` reply, already reduced to logical
/// coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HyprMonitor {
    pub id: i64,
    pub name: String,
    /// Global logical position (`... at XxY`).
    pub x: i32,
    pub y: i32,
}

/// The raw `monitors` reply — same transport strategy as
/// [`active_window_reply`].
pub(crate) fn monitors_reply() -> Option<String> {
    if let Some(p) = socket_path() {
        match socket_request(&p, "monitors") {
            Ok(reply) if !reply.trim_start().starts_with("unknown request") => {
                return Some(reply);
            }
            Ok(reply) => debug!(%reply, "hypr socket refused monitors; using hyprctl"),
            Err(e) => debug!(?e, "hypr socket monitors failed; using hyprctl"),
        }
    }
    let out = Command::new("hyprctl").arg("monitors").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse the plain-text `monitors` reply. Block headers look like
/// `Monitor eDP-1 (ID 0):`; the next line carries the mode and the
/// global position: `2560x1440@165.00000 at 0x0`. Only the name, id
/// and position are extracted — the tooltip needs an output name and
/// an origin for output-local margins, nothing more (mode size is
/// physical pixels and would need scale/transform arithmetic to be
/// useful; the compositor clamps overhanging margins anyway).
pub(crate) fn parse_monitors(reply: &str) -> Vec<HyprMonitor> {
    let mut out = Vec::new();
    let mut current: Option<(i64, String)> = None;
    for line in reply.lines() {
        if let Some(rest) = line.strip_prefix("Monitor ") {
            // `NAME (ID N):`
            current = None;
            if let Some((name, id_part)) = rest.split_once(" (ID ") {
                if let Some(id) = id_part
                    .trim_end()
                    .strip_suffix("):")
                    .and_then(|s| s.trim().parse::<i64>().ok())
                {
                    current = Some((id, name.trim().to_owned()));
                }
            }
        } else if let Some((id, name)) = current.as_ref() {
            // First body line: `WxH@RR at XxY` (indented).
            let t = line.trim();
            if let Some((_, pos)) = t.split_once(" at ") {
                if let Some((x, y)) = parse_pair(pos, 'x') {
                    out.push(HyprMonitor {
                        id: *id,
                        name: name.clone(),
                        x: x as i32,
                        y: y as i32,
                    });
                }
                current = None; // one position line per block
            }
        }
    }
    out
}

/// The global pointer position (`cursorpos` request) — the tooltip's
/// caret proxy. Same transport strategy as [`active_window_reply`].
pub(crate) fn cursor_position() -> Option<(i32, i32)> {
    let reply = if let Some(p) = socket_path() {
        match socket_request(&p, "cursorpos") {
            Ok(reply) if !reply.trim_start().starts_with("unknown request") => Some(reply),
            Ok(reply) => {
                debug!(%reply, "hypr socket refused cursorpos; using hyprctl");
                None
            }
            Err(e) => {
                debug!(?e, "hypr socket cursorpos failed; using hyprctl");
                None
            }
        }
    } else {
        None
    };
    let reply = match reply {
        Some(r) => r,
        None => {
            let out = Command::new("hyprctl").arg("cursorpos").output().ok()?;
            if !out.status.success() {
                return None;
            }
            String::from_utf8_lossy(&out.stdout).into_owned()
        }
    };
    let (x, y) = parse_pair(reply.trim(), ',')?;
    Some((x as i32, y as i32))
}

/// `"11, 22"` with the given separator → `(11, 22)`.
fn parse_pair(v: &str, sep: char) -> Option<(i64, i64)> {
    let (a, b) = v.trim().split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}
