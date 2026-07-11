//! `HyprlandSwitcher` — layout control via hyprctl.

use super::*;
use crate::linux::shared::{cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, warn};

pub struct HyprlandSwitcher;

pub fn try_init() -> Option<HyprlandSwitcher> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    if !cmd_exists("hyprctl") {
        warn!("HYPRLAND_INSTANCE_SIGNATURE set but hyprctl not in PATH");
        return None;
    }
    Some(HyprlandSwitcher)
}

impl LayoutSwitcher for HyprlandSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        // Parse `hyprctl devices` and ask `choose_current_keymap`
        // which keyboard actually reflects the user's typing layout
        // (remapper virtual keyboard > `main: yes` > first; our own
        // emitter is never eligible). Trusting `main` alone is not
        // enough: Hyprland re-elects `main` when devices appear, and
        // right after our emitter registers it often *is* the
        // emitter — whose keymap only tracks `switchxkblayout all`,
        // never the user's per-device Alt+Shift toggle. Reading it
        // desyncs the engine from the real keystream, which kills
        // exactly one direction of correction (the "uk→en works but
        // en→uk never fires" report).
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
        // Use `all` rather than `main-keyboard`: in setups with
        // `keyd` (or any input remapper that creates its own uinput
        // device), the physical keystroke stream actually reaches
        // the compositor through the remapper's virtual keyboard,
        // which Hyprland sees as a separate xkb context. Switching
        // only `main-keyboard` would flip our own virtual device
        // and leave the keyd-proxied one on the old layout — at
        // which point a replay through uinput re-types the
        // original Latin glyphs and you get the "blink and stay the
        // same" symptom. `all` keeps every device in lock-step.
        let _ = request(&["switchxkblayout", "all", &idx.to_string()])?;
        debug!(layout = %id, idx, "Hyprland layout switched (all devices)");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-hyprland-hyprctl"
    }
}
