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

use super::shared::{cmd_exists, xkb_to_bcp47};

/// Name our uinput emitter registers itself under (see
/// `kb-input`'s `UinputEmitter`). We skip it when reading the active
/// layout because it never receives the user's manual Alt+Shift
/// toggle — see `current()`.
const EMITTER_DEVICE_NAME: &str = "kb-switcher virtual keyboard";

/// Map a Hyprland `active keymap` description (e.g. `"Ukrainian"`,
/// `"English (US)"`) to a BCP-47 `LayoutId`.
fn keymap_to_layout(name: &str) -> LayoutId {
    let xkb = name_to_xkb_code(name);
    let bcp = xkb_to_bcp47(&xkb).map(str::to_owned).unwrap_or(xkb);
    LayoutId::new(bcp)
}

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
        // Parse `hyprctl devices` block-by-block and read the keymap of
        // the keyboard Hyprland flags `main: yes`.
        //
        // The previous "first active keymap line wins" approach was
        // wrong on this class of setup: with `keyd` (or any remapper)
        // the real keystroke stream — and the per-device
        // `grp:*_toggle` layout switch the user triggers with
        // Alt+Shift — lands on the remapper's *virtual* keyboard,
        // while the physical Logitech / power-button / sleep-button
        // devices keep their stale layout. The first device printed is
        // usually one of those stale ones, so we'd report en-US while
        // the user is actually typing in uk-UA. Hyprland's `main`
        // keyboard tracks the device that input is really flowing
        // through, which is exactly what we want.
        //
        // We deliberately skip our own uinput emitter device: when it
        // exists Hyprland sometimes promotes it to `main`, but it
        // never receives the user's Alt+Shift toggle (we drive it only
        // via `switchxkblayout all`), so trusting it would reintroduce
        // the desync.
        let out = run("hyprctl", &["devices"])?;
        let mut cur_name: Option<String> = None;
        let mut cur_keymap: Option<String> = None;
        let mut fallback: Option<String> = None;
        let mut expect_name = false;
        for raw in out.lines() {
            let line = raw.trim();
            if line.starts_with("Keyboard at") {
                cur_name = None;
                cur_keymap = None;
                expect_name = true;
            } else if expect_name {
                cur_name = Some(line.to_owned());
                expect_name = false;
            } else if let Some(rest) = line.strip_prefix("active keymap:") {
                let km = rest.trim().to_owned();
                if cur_name.as_deref() != Some(EMITTER_DEVICE_NAME) && fallback.is_none() {
                    fallback = Some(km.clone());
                }
                cur_keymap = Some(km);
            } else if line == "main: yes" && cur_name.as_deref() != Some(EMITTER_DEVICE_NAME) {
                if let Some(km) = cur_keymap.take() {
                    return Ok(keymap_to_layout(&km));
                }
            }
        }
        if let Some(km) = fallback {
            return Ok(keymap_to_layout(&km));
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
        let _ = run("hyprctl", &["switchxkblayout", "all", &idx.to_string()])?;
        debug!(layout = %id, idx, "Hyprland layout switched (all devices)");
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
