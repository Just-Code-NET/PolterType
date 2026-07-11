//! `KdeSwitcher` — layout control via qdbus / kded.

use super::*;
use crate::linux::shared::{bcp47_to_xkb, cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct KdeSwitcher {
    qdbus: &'static str,
}

pub fn try_init() -> Option<KdeSwitcher> {
    // `XDG_CURRENT_DESKTOP=KDE` is authoritative. `KDE_FULL_SESSION`
    // can leak into non-KDE sessions (a user on Hyprland/Sway with
    // KDE/Plasma installed for the Qt theming stack will have it set
    // to "true" without actually running KWin), so we don't trust it
    // alone — it would mis-activate this backend on Hyprland where
    // the Hyprland switcher is the one that actually works.
    let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|s| s.to_uppercase().contains("KDE"))
        .unwrap_or(false);
    if !is_kde {
        return None;
    }
    let candidate = if cmd_exists("qdbus6") {
        KdeSwitcher { qdbus: "qdbus6" }
    } else if cmd_exists("qdbus") {
        KdeSwitcher { qdbus: "qdbus" }
    } else {
        warn!("XDG_CURRENT_DESKTOP=KDE but neither qdbus6 nor qdbus is in PATH");
        return None;
    };
    // Probe the actual D-Bus service — if `org.kde.keyboard` isn't on
    // the bus the daemon (`kded6`/`plasma-keyboard`) isn't running,
    // and every subsequent call would just fail. Better to fall
    // through to the next backend now.
    if candidate.list_active().is_err() {
        debug!(
            qdbus = candidate.qdbus,
            "KDE qdbus reachable but org.kde.keyboard not responding"
        );
        return None;
    }
    Some(candidate)
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
