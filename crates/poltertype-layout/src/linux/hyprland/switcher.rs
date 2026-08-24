//! `HyprlandSwitcher` — layout control via hyprctl.

use super::*;
use crate::linux::shared::{cmd_exists, keymap_to_layout, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, warn};

pub struct HyprlandSwitcher;

pub fn try_init() -> Option<HyprlandSwitcher> {
    // A live socket rather than `HYPRLAND_INSTANCE_SIGNATURE`: an
    // autostarted process can be running under Hyprland with the
    // variable unset — see `ipc::instance_signature`.
    ipc::socket_path()?;
    if !cmd_exists("hyprctl") {
        warn!("HYPRLAND_INSTANCE_SIGNATURE set but hyprctl not in PATH");
        return None;
    }
    Some(HyprlandSwitcher)
}

impl LayoutSwitcher for HyprlandSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        // Trusting `main: yes` alone is not enough: Hyprland re-elects
        // it when devices appear, and right after our emitter registers
        // it often *is* the emitter — see `choose_current_keymap`.
        let out = request(&["devices"])?;
        let keyboards = parse_keyboards(&out);
        choose_current_keymap(&keyboards)
            .map(keymap_to_layout)
            .ok_or_else(|| {
                LayoutError::Os(
                    "could not find an 'active keymap' line in `hyprctl devices`".into(),
                )
            })
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        // `hyprctl getoption input:kb_layout` → e.g. `string: us,ua`.
        // NB: `kb_layout` is Hyprland's own option name — it has
        // nothing to do with the app's former "kb-switcher" branding
        // and must never be caught by a rename sweep again.
        let out = request(&["getoption", "input:kb_layout"])?;
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
        // `all`, not `main-keyboard`: behind an input remapper the
        // physical keystrokes reach the compositor through the
        // remapper's virtual keyboard, which Hyprland treats as a
        // separate xkb context. Switching only `main-keyboard` flips
        // our own device and leaves the proxied one on the old layout,
        // so a replay re-types the original glyphs — the "blink and stay
        // the same" symptom.
        let _ = request(&["switchxkblayout", "all", &idx.to_string()])?;
        debug!(layout = %id, idx, "Hyprland layout switched (all devices)");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-hyprland-hyprctl"
    }
}
