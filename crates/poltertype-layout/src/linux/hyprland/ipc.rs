//! hyprctl invocation and the Hyprland IPC socket.

use super::*;
use crate::linux::shared::{cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use tracing::{debug, warn};

pub(crate) fn run(prog: &str, args: &[&str]) -> Result<String, LayoutError> {
    let out = Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| LayoutError::Os(format!("{prog}: {e}")))?;
    if !out.status.success() {
        return Err(LayoutError::Os(format!(
            "{prog} {args:?} exited {}",
            out.status
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
pub(crate) fn instance_signature() -> Option<OsString> {
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
pub(crate) fn socket_path() -> Option<PathBuf> {
    let sig = instance_signature()?;
    signature_dirs()
        .into_iter()
        .map(|d| d.join(&sig).join(".socket.sock"))
        .find(|p| p.exists())
}

/// One request over Hyprland's IPC socket: write the command, read
/// the reply to EOF. This is exactly what `hyprctl` does under the
/// hood — minus the ~20-60 ms of process spawn, which matters because
/// the engine queries the layout on the hot keystroke path.
pub(crate) fn socket_request(path: &Path, cmd: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(path)?;
    s.set_read_timeout(Some(Duration::from_millis(400)))?;
    s.set_write_timeout(Some(Duration::from_millis(400)))?;
    s.write_all(cmd.as_bytes())?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

/// Issue a Hyprland command: IPC socket first, `hyprctl` subprocess
/// as the fallback (covers exotic setups where the socket moved or a
/// sandbox blocks UNIX sockets but allows exec).
pub(crate) fn request(args: &[&str]) -> Result<String, LayoutError> {
    if let Some(p) = socket_path() {
        let joined = args.join(" ");
        match socket_request(&p, &joined) {
            Ok(reply) if !reply.trim_start().starts_with("unknown request") => {
                return Ok(reply);
            }
            Ok(reply) => {
                debug!(%reply, cmd = %joined, "hypr socket refused request; using hyprctl")
            }
            Err(e) => debug!(?e, cmd = %joined, "hypr socket request failed; using hyprctl"),
        }
    }
    run("hyprctl", args)
}
