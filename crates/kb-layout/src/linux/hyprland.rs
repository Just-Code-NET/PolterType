//! Hyprland layout switcher via `hyprctl`.
//!
//! Hyprland (a tiling Wayland compositor) exposes IPC over a UNIX
//! socket; the canonical user-facing tool is `hyprctl`. Layout config
//! looks like `kb_layout = us,ua` in `hyprland.conf`; switching is by
//! integer index into that list, scoped to a specific keyboard
//! device.
//!
//! Activation: probe `HYPRLAND_INSTANCE_SIGNATURE` — Hyprland sets it
//! on every spawned process.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::process::Command;

use tracing::{debug, warn};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

use super::shared::xkb_to_bcp47;

pub struct HyprlandSwitcher;

pub fn try_init() -> Option<HyprlandSwitcher> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return None;
    }
    if !cmd_exists("hyprctl") {
        warn!("HYPRLAND_INSTANCE_SIGNATURE set but hyprctl not in PATH");
        return None;
    }
    Some(HyprlandSwitcher)
}

impl LayoutSwitcher for HyprlandSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        // `hyprctl devices -j` returns JSON with .keyboards[].active_keymap.
        // We avoid pulling in `serde_json` and instead grep for the
        // first `"active_keymap"` line on the main keyboard. Good
        // enough for v0.1; a structured parse comes if it ever
        // misbehaves.
        let out = run("hyprctl", &["devices"])?;
        for line in out.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("active keymap:") {
                let name = rest.trim();
                let xkb = name_to_xkb_code(name);
                return Ok(LayoutId::new(xkb_to_bcp47(&xkb).unwrap_or(xkb).to_owned()));
            }
        }
        Err(LayoutError::Os(
            "could not find an 'active keymap' line in `hyprctl devices`".into(),
        ))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        // `hyprctl getoption input:kb_layout` → e.g. `string: us,ua`.
        let out = run("hyprctl", &["getoption", "input:kb_layout"])?;
        for line in out.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("str:") {
                return Ok(parse_csv(rest.trim()));
            }
            if let Some(rest) = line.strip_prefix("string:") {
                return Ok(parse_csv(rest.trim()));
            }
        }
        Err(LayoutError::Os(
            "could not parse `hyprctl getoption input:kb_layout` output".into(),
        ))
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let layouts = self.list_active()?;
        let Some(idx) = layouts.iter().position(|l| l == id) else {
            return Err(LayoutError::NotActive(id.clone()));
        };
        // Hyprland needs a device name; "main-keyboard" is the
        // canonical alias for the primary keyboard.
        let _ = run(
            "hyprctl",
            &["switchxkblayout", "main-keyboard", &idx.to_string()],
        )?;
        debug!(layout = %id, idx, "Hyprland layout switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-hyprland-hyprctl"
    }
}

fn parse_csv(s: &str) -> Vec<LayoutId> {
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|code| LayoutId::new(xkb_to_bcp47(code).unwrap_or(code).to_owned()))
        .collect()
}

/// `"English (US)" → "us"` — Hyprland's `active keymap` uses the
/// pretty XKB description; we don't have a full table, so we take a
/// best-effort guess.
fn name_to_xkb_code(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.as_str() {
        s if s.contains("ukrain") => "ua".into(),
        s if s.contains("english") || s.contains("us") => "us".into(),
        s if s.contains("russian") => "ru".into(),
        s if s.contains("german") => "de".into(),
        s if s.contains("french") => "fr".into(),
        _ => name.to_owned(),
    }
}

fn cmd_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run(prog: &str, args: &[&str]) -> Result<String, LayoutError> {
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
