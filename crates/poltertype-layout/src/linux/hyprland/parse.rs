//! hyprctl output parsing and layout-name mapping.

use super::*;
use crate::linux::shared::{cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, warn};

/// Map a Hyprland `active keymap` description (e.g. `"Ukrainian"`,
/// `"English (US)"`) to a BCP-47 `LayoutId`.
pub(crate) fn keymap_to_layout(name: &str) -> LayoutId {
    let xkb = name_to_xkb_code(name);
    let bcp = xkb_to_bcp47(&xkb).map(str::to_owned).unwrap_or(xkb);
    LayoutId::new(bcp)
}

/// Hyprland prints device names normalised: lowercase, spaces and
/// other separators collapsed to dashes (`poltertype virtual
/// keyboard` → `poltertype-virtual-keyboard`). Apply the same shape
/// to both sides before comparing.
pub(crate) fn normalize_device_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// Parse the keyboard blocks out of `hyprctl devices` text output.
pub(crate) fn parse_keyboards(out: &str) -> Vec<KeyboardBlock> {
    let mut keyboards = Vec::new();
    let mut expect_name = false;
    for raw in out.lines() {
        let line = raw.trim();
        if line.starts_with("Keyboard at") {
            expect_name = true;
        } else if expect_name {
            keyboards.push(KeyboardBlock {
                name: line.to_owned(),
                keymap: None,
                main: false,
            });
            expect_name = false;
        } else if let Some(rest) = line.strip_prefix("active keymap:") {
            if let Some(kb) = keyboards.last_mut() {
                kb.keymap = Some(rest.trim().to_owned());
            }
        } else if line == "main: yes" {
            if let Some(kb) = keyboards.last_mut() {
                kb.main = true;
            }
        }
    }
    keyboards
}

/// Pick the keyboard whose keymap reflects what the user is actually
/// typing in. Our own uinput emitter is never eligible — it only sees
/// `switchxkblayout all`, never the user's per-device toggle, so
/// trusting it desyncs the engine from the real keystream.
///
/// In order: an input-remapper virtual keyboard (keyd, kanata,
/// kmonad), through which both the keystream and the toggle flow; then
/// the device Hyprland flags `main: yes`; then the first keyboard with
/// a keymap.
pub(crate) fn choose_current_keymap(keyboards: &[KeyboardBlock]) -> Option<&str> {
    let emitter = normalize_device_name(EMITTER_DEVICE_NAME);
    let eligible: Vec<&KeyboardBlock> = keyboards
        .iter()
        .filter(|kb| kb.keymap.is_some() && normalize_device_name(&kb.name) != emitter)
        .collect();

    let remapper = eligible.iter().find(|kb| {
        let name = normalize_device_name(&kb.name);
        REMAPPER_NAME_MARKERS.iter().any(|m| name.contains(m))
    });
    remapper
        .or_else(|| eligible.iter().find(|kb| kb.main))
        .or_else(|| eligible.first())
        .and_then(|kb| kb.keymap.as_deref())
}

pub(crate) fn parse_csv(s: &str) -> Vec<LayoutId> {
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|code| LayoutId::new(xkb_to_bcp47(code).unwrap_or(code).to_owned()))
        .collect()
}

/// `"English (US)" → "us"` — Hyprland's `active keymap` uses the
/// pretty XKB description; we don't have a full table, so we take a
/// best-effort guess.
pub(crate) fn name_to_xkb_code(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.as_str() {
        s if s.contains("ukrain") => "ua".into(),
        // `us` must be an exact match, not a substring — "Russian"
        // and "Belarusian" both contain the letters "us", and the
        // broad check made every Russian keymap resolve to en-US.
        s if s.contains("english") || s == "us" => "us".into(),
        s if s.contains("russian") => "ru".into(),
        s if s.contains("german") => "de".into(),
        s if s.contains("french") => "fr".into(),
        // Every language we bundle a wordlist for must resolve here, or
        // the moment the user switches to it the engine sees an unknown
        // current layout: empty renders, phantom re-corrections.
        s if s.contains("spanish") => "es".into(),
        _ => name.to_owned(),
    }
}
