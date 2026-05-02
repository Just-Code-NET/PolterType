//! GSettings-based layout switcher.
//!
//! Despite the file name, this covers every DE that exposes the
//! `org.gnome.desktop.input-sources` schema, which is the lingua
//! franca for GNOME-derivative environments: **GNOME**, **Ubuntu
//! Unity 7+**, **Cinnamon**, **Budgie**, **Pantheon** (elementary
//! OS), **MATE** (when configured via gsettings).
//!
//! The schema's `sources` is an array of `(type, id)` pairs (typically
//! `('xkb', 'us')` etc.) and `current` is a `u` index into it.
//! Switching = writing a new `current`.
//!
//! `try_init()` is a strict probe — it requires both `gsettings` in
//! `$PATH` *and* a successful read of `sources` from the schema. So
//! KDE / Hyprland / IBus / Fcitx-only sessions correctly fall through
//! to their own backends instead of being claimed here.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::process::Command;

use tracing::{debug, warn};

use super::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};

const SCHEMA: &str = "org.gnome.desktop.input-sources";

pub struct GnomeSwitcher;

pub fn try_init() -> Option<GnomeSwitcher> {
    // Reject if `gsettings` is not in PATH at all.
    let exists = Command::new("gsettings")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !exists {
        return None;
    }
    // Reject if the schema is not installed (most KDE / minimal-DE
    // systems). `gsettings get` exits non-zero if the schema or key
    // is missing.
    let ok = Command::new("gsettings")
        .args(["get", SCHEMA, "sources"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    Some(GnomeSwitcher)
}

impl LayoutSwitcher for GnomeSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let sources = read_sources()?;
        let idx = read_current_index()?;
        sources
            .get(idx as usize)
            .cloned()
            .ok_or_else(|| LayoutError::Os(format!("current index {idx} out of range")))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        read_sources()
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let sources = read_sources()?;
        let Some(idx) = sources.iter().position(|s| s == id) else {
            return Err(LayoutError::NotActive(id.clone()));
        };
        // We use `gsettings set` to write — bypasses the need to bring
        // in a full GSettings client.
        let status = Command::new("gsettings")
            .args(["set", SCHEMA, "current", &idx.to_string()])
            .status()
            .map_err(|e| LayoutError::Os(format!("gsettings spawn: {e}")))?;
        if !status.success() {
            return Err(LayoutError::Os(format!("gsettings set returned {status}")));
        }
        debug!(layout = %id, idx, "GNOME layout switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        // "gsettings" is the honest backend tag — it's what we shell
        // out to. Picked up by GNOME / Unity / Cinnamon / Budgie /
        // Pantheon / MATE.
        "linux-gsettings"
    }
}

fn read_sources() -> Result<Vec<LayoutId>, LayoutError> {
    let out = Command::new("gsettings")
        .args(["get", SCHEMA, "sources"])
        .output()
        .map_err(|e| LayoutError::Os(format!("gsettings get: {e}")))?;
    if !out.status.success() {
        return Err(LayoutError::Os(format!(
            "gsettings get exited {}",
            out.status
        )));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(parse_sources(&s))
}

fn read_current_index() -> Result<u32, LayoutError> {
    let out = Command::new("gsettings")
        .args(["get", SCHEMA, "current"])
        .output()
        .map_err(|e| LayoutError::Os(format!("gsettings get current: {e}")))?;
    if !out.status.success() {
        return Err(LayoutError::Os(format!(
            "gsettings get current exited {}",
            out.status
        )));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    // Output looks like `uint32 0\n`.
    s.trim()
        .strip_prefix("uint32")
        .map(str::trim)
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| LayoutError::Os(format!("could not parse current index from {s:?}")))
}

/// Parse the gvariant-ish text returned by `gsettings get`. Format:
/// `[('xkb', 'us'), ('xkb', 'ua')]`. We don't need a full parser — the
/// second element of each tuple is the layout code we want, and we
/// translate `xkb` codes to BCP-47 with a small table.
fn parse_sources(raw: &str) -> Vec<LayoutId> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '(' {
            continue;
        }
        // (type, id) — read two single-quoted strings.
        let _type = take_quoted(&mut chars);
        let id = take_quoted(&mut chars);
        if let Some(id) = id {
            out.push(LayoutId::new(
                xkb_to_bcp47(&id).map(str::to_owned).unwrap_or(id),
            ));
        }
    }
    out
}

fn take_quoted<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) -> Option<String> {
    // Skip until next single quote.
    for c in chars.by_ref() {
        if c == '\'' {
            break;
        }
    }
    let mut s = String::new();
    for c in chars.by_ref() {
        if c == '\'' {
            return Some(s);
        }
        s.push(c);
    }
    None
}
