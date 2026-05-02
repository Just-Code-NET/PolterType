//! GNOME layout switcher via D-Bus + GSettings.
//!
//! GNOME stores the active input source in
//! `org.gnome.desktop.input-sources` (GSettings). The `current` key
//! is a `u` index into the `sources` array (each entry is
//! `(type, id)` — typically `(s, s)`).
//!
//! Querying the array of sources requires reading the GSettings
//! value; we use the `org.freedesktop.portal.Settings` interface
//! when available, falling back to `gsettings get` shell-out as a
//! v0.1.x convenience.
//!
//! Switching = setting `current` to the index of the desired layout.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::process::Command;

use tracing::{debug, info, warn};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

use crate::{LayoutError, LayoutId, LayoutSwitcher};

const SCHEMA: &str = "org.gnome.desktop.input-sources";

pub struct GnomeSwitcher {
    conn: Connection,
}

pub fn try_init() -> Option<GnomeSwitcher> {
    let conn = Connection::session().ok()?;
    // Cheap reachability ping: org.freedesktop.DBus.GetId on the bus.
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .ok()?;
    let _: String = proxy.call("GetId", &()).ok()?;
    Some(GnomeSwitcher { conn })
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
        "linux-gnome-gsettings"
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
            out.push(LayoutId::new(xkb_to_bcp47(&id).unwrap_or(id)));
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

fn xkb_to_bcp47(code: &str) -> Option<String> {
    Some(
        match code {
            "us" => "en-US",
            "gb" => "en-GB",
            "ua" => "uk-UA",
            "ru" => "ru-RU",
            "de" => "de-DE",
            "fr" => "fr-FR",
            "es" => "es-ES",
            "pl" => "pl-PL",
            "gr" => "el-GR",
            _ => return None,
        }
        .to_owned(),
    )
}
