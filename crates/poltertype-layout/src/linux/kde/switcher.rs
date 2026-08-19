//! `KdeSwitcher` — layout control via qdbus / kded.

use super::parse::layout_short_names;
use super::*;
use crate::linux::shared::{bcp47_to_xkb, cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct KdeSwitcher {
    qdbus: &'static str,
    /// Plasma ≥ 5.23 addresses layouts by **position in the configured
    /// list**: `getLayout() -> uint`, `setLayout(uint) -> bool`. Older
    /// Plasma passed the xkb short name as a string. Probed once at
    /// init from the shape of `getLayout`'s answer, because guessing
    /// wrong here does not fail loudly — it switches to the wrong
    /// layout, or to none.
    indexed: bool,
}

/// qdbus under every name distributions give it. Qt 6 first: on a
/// Plasma 6 session the Qt 5 binary may still be installed and talks to
/// the same bus, but the Qt 6 one is the supported pairing.
const QDBUS_BINARIES: [&str; 4] = ["qdbus6", "qdbus-qt6", "qdbus", "qdbus-qt5"];

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
    let Some(qdbus) = QDBUS_BINARIES.into_iter().find(|b| cmd_exists(b)) else {
        warn!(
            candidates = ?QDBUS_BINARIES,
            "XDG_CURRENT_DESKTOP=KDE but no qdbus binary is in PATH"
        );
        return None;
    };
    let candidate = KdeSwitcher {
        qdbus,
        indexed: true,
    };

    // Probe the actual D-Bus service — if `org.kde.keyboard` isn't on
    // the bus the daemon (`kded6`/KWin) isn't running, and every
    // subsequent call would just fail. Better to fall through to the
    // next backend now.
    //
    // The probe demands a *non-empty parsed* list, not merely a
    // successful exit: qdbus answers an un-renderable return type with
    // an error sentence on stdout and exit 0, and the old
    // exit-status-only probe accepted that as a layout list (#31).
    let names = match candidate.short_names() {
        Ok(names) => names,
        Err(e) => {
            debug!(
                qdbus,
                ?e,
                "KDE qdbus present but org.kde.keyboard did not answer with a layout list"
            );
            return None;
        }
    };

    let indexed = candidate.probe_indexed_api();
    debug!(qdbus, ?names, indexed, "KDE layout backend ready");
    Some(KdeSwitcher { qdbus, indexed })
}

impl KdeSwitcher {
    fn call(&self, method: &str, args: &[&str]) -> Result<String, LayoutError> {
        let mut argv = vec![SERVICE, OBJECT, method];
        argv.extend_from_slice(args);
        run(self.qdbus, &argv)
    }

    /// xkb short names of the configured layouts, in Plasma's own order.
    fn short_names(&self) -> Result<Vec<String>, LayoutError> {
        let raw = run_literal(
            self.qdbus,
            &[SERVICE, OBJECT, "org.kde.KeyboardLayouts.getLayoutsList"],
        )?;
        let names = layout_short_names(&raw);
        if names.is_empty() {
            return Err(LayoutError::Os(format!(
                "getLayoutsList returned nothing parseable: {}",
                raw.trim()
            )));
        }
        Ok(names)
    }

    /// Does `getLayout` answer with an index (Plasma ≥ 5.23) or with the
    /// short name itself? xkb short names are never numeric, so the
    /// shape of one answer settles it. Assumes the modern API when the
    /// call fails outright — that is the one every supported Plasma
    /// speaks.
    fn probe_indexed_api(&self) -> bool {
        match self.call("org.kde.KeyboardLayouts.getLayout", &[]) {
            Ok(raw) => raw.trim().parse::<u32>().is_ok(),
            Err(_) => true,
        }
    }
}

impl LayoutSwitcher for KdeSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let raw = self.call("org.kde.KeyboardLayouts.getLayout", &[])?;
        let raw = raw.trim();
        let code = if self.indexed {
            let index: usize = raw.parse().map_err(|_| {
                LayoutError::Os(format!("getLayout answered {raw:?}, expected an index"))
            })?;
            self.short_names()?.into_iter().nth(index).ok_or_else(|| {
                LayoutError::Os(format!("getLayout index {index} is out of range"))
            })?
        } else {
            raw.to_owned()
        };
        Ok(LayoutId::new(
            xkb_to_bcp47(&code).unwrap_or(&code).to_owned(),
        ))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        Ok(self
            .short_names()?
            .into_iter()
            .map(|code| LayoutId::new(xkb_to_bcp47(&code).unwrap_or(&code).to_owned()))
            .collect())
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let target = bcp47_to_xkb(id.as_str()).unwrap_or(id.as_str());
        let arg = if self.indexed {
            let names = self.short_names()?;
            let index = names.iter().position(|n| n == target).ok_or_else(|| {
                LayoutError::Unsupported(format!(
                    "{id} ({target}) is not among the KDE layouts {names:?}"
                ))
            })?;
            index.to_string()
        } else {
            target.to_owned()
        };
        let _ = self.call("org.kde.KeyboardLayouts.setLayout", &[&arg])?;
        debug!(layout = %id, arg, "KDE layout switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-kde-qdbus"
    }
}
