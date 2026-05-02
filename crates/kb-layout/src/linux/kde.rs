//! KDE Plasma layout switcher via `qdbus`.
//!
//! KDE Plasma 5+ exposes layout state on the session bus at
//! `org.kde.keyboard /Layouts (org.kde.KeyboardLayouts)`:
//!
//! * `getLayoutsList() → as`
//! * `getLayout() → s` (BCP-47-style short name like `"us"` / `"ua"`)
//! * `setLayout(s) → b`
//!
//! Plasma 6 ships `qdbus6`; older systems have `qdbus` (Qt5). We try
//! both. Activation: `XDG_CURRENT_DESKTOP` contains `"KDE"` or
//! `KDE_FULL_SESSION=true`.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::process::Command;

use tracing::{debug, warn};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

use super::shared::{bcp47_to_xkb, xkb_to_bcp47};

pub struct KdeSwitcher {
    qdbus: &'static str,
}

pub fn try_init() -> Option<KdeSwitcher> {
    let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|s| s.to_uppercase().contains("KDE"))
        .unwrap_or(false)
        || std::env::var("KDE_FULL_SESSION").is_ok();
    if !is_kde {
        return None;
    }
    if cmd_exists("qdbus6") {
        return Some(KdeSwitcher { qdbus: "qdbus6" });
    }
    if cmd_exists("qdbus") {
        return Some(KdeSwitcher { qdbus: "qdbus" });
    }
    warn!("XDG_CURRENT_DESKTOP=KDE but neither qdbus6 nor qdbus is in PATH");
    None
}

impl LayoutSwitcher for KdeSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let raw = run(
            self.qdbus,
            &[
                "org.kde.keyboard",
                "/Layouts",
                "org.kde.KeyboardLayouts.getLayout",
            ],
        )?;
        let code = raw.trim();
        Ok(LayoutId::new(xkb_to_bcp47(code).unwrap_or(code).to_owned()))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        let raw = run(
            self.qdbus,
            &[
                "org.kde.keyboard",
                "/Layouts",
                "org.kde.KeyboardLayouts.getLayoutsList",
            ],
        )?;
        Ok(raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|code| LayoutId::new(xkb_to_bcp47(code).unwrap_or(code).to_owned()))
            .collect())
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let target = bcp47_to_xkb(id.as_str()).unwrap_or(id.as_str());
        let _ = run(
            self.qdbus,
            &[
                "org.kde.keyboard",
                "/Layouts",
                "org.kde.KeyboardLayouts.setLayout",
                target,
            ],
        )?;
        debug!(layout = %id, "KDE layout switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-kde-qdbus"
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
