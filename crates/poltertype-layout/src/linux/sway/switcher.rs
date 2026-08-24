//! `SwaySwitcher` — layout control via swaymsg.

use super::*;
use crate::linux::shared::cmd_exists;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct SwaySwitcher;

/// A live sway IPC socket, not the compositor's name. `SWAYSOCK` is
/// exported to a session's children but an autostarted process can be
/// running under sway without it — the same trap `hyprland::try_init`
/// documents — so the socket in `XDG_RUNTIME_DIR` is checked too.
pub fn try_init() -> Option<SwaySwitcher> {
    if !cmd_exists("swaymsg") {
        return None;
    }
    if std::env::var_os("SWAYSOCK").is_none() && socket_path().is_none() {
        return None;
    }
    // Answering at all is the probe: `swaymsg` exits non-zero with
    // "Unable to retrieve socket path" when no sway is listening.
    run(&["-t", "get_version"]).ok()?;
    Some(SwaySwitcher)
}

/// `$XDG_RUNTIME_DIR/sway-ipc.<uid>.<pid>.sock`, when `SWAYSOCK` is not
/// in this process's environment.
pub(crate) fn socket_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
        })
}

pub(crate) fn run(args: &[&str]) -> Result<String, LayoutError> {
    let mut cmd = Command::new("swaymsg");
    if std::env::var_os("SWAYSOCK").is_none()
        && let Some(sock) = socket_path()
    {
        cmd.env("SWAYSOCK", sock);
    }
    let out = cmd
        .args(args)
        .output()
        .map_err(|e| LayoutError::Os(format!("swaymsg spawn: {e}")))?;
    if !out.status.success() {
        return Err(LayoutError::Os(format!(
            "swaymsg {args:?} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn keyboard() -> Result<SwayKeyboard, LayoutError> {
    Ok(parse_inputs(&run(&["-t", "get_inputs"])?))
}

impl LayoutSwitcher for SwaySwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let kb = keyboard()?;
        let idx = kb
            .active
            .ok_or_else(|| LayoutError::Os("sway named no active layout".into()))?;
        kb.layouts
            .get(idx)
            .cloned()
            .ok_or_else(|| LayoutError::Os(format!("sway's active index {idx} is out of range")))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        Ok(keyboard()?.layouts)
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let layouts = self.list_active()?;
        let Some(idx) = layouts.iter().position(|l| l == id) else {
            return Err(LayoutError::NotActive(id.clone()));
        };
        // `type:keyboard`, not one identifier: sway keeps xkb state per
        // input, and behind an input remapper the user's keystrokes
        // arrive through a different device than the one we would have
        // picked. The same reasoning as Hyprland's `all`.
        run(&[&format!("input type:keyboard xkb_switch_layout {idx}")])?;
        debug!(layout = %id, idx, "sway layout switched (every keyboard)");
        Ok(())
    }

    /// sway answers from its own input state, which is what it applies
    /// to the keyboard — not a setting we wrote and read back.
    fn verify_switched(&self, target: &LayoutId) -> Option<bool> {
        Some(self.current().is_ok_and(|now| now == *target))
    }

    fn backend_name(&self) -> &'static str {
        "linux-sway-swaymsg"
    }
}
