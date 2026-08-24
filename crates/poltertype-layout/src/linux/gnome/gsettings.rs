//! gsettings reads and input-sources parsing.

use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub(crate) fn read_sources() -> Result<Vec<LayoutId>, LayoutError> {
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

/// The input source GNOME is **actually on**, as opposed to the one we
/// asked for.
///
/// `mru-sources` is most-recently-used order, so its head is the live
/// source — and unlike `current` it is maintained by the shell itself.
/// Measured on GNOME 49 (Ubuntu 26.04), 2026-08-24: switching with the
/// desktop's own shortcut moves this list and leaves `current`
/// untouched, while writing `current` moves neither the list nor the
/// keyboard.
///
/// Never written by us. A write would put a value here that the shell
/// did not choose, and this is the only reading that can contradict our
/// own write — spending it would leave nothing to check against.
pub(crate) fn read_live_source() -> Option<LayoutId> {
    let out = Command::new("gsettings")
        .args(["get", SCHEMA, "mru-sources"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_sources(&String::from_utf8_lossy(&out.stdout))
        .into_iter()
        .next()
}

/// The shortcut GNOME binds to "switch to the next input source".
///
/// `gsettings` prints a list — `['<Super>space', 'XF86Keyboard']` — and
/// only an entry we can actually name a scancode for is any use; the
/// media keys among them are skipped rather than guessed at.
pub(crate) fn read_switch_binding() -> Option<poltertype_types::SwitchChord> {
    let out = Command::new("gsettings")
        .args([
            "get",
            "org.gnome.desktop.wm.keybindings",
            "switch-input-source",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .split('\'')
        .find_map(crate::linux::chord::parse_gtk_accelerator)
}

pub(crate) fn read_current_index() -> Result<u32, LayoutError> {
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
pub(crate) fn parse_sources(raw: &str) -> Vec<LayoutId> {
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

pub(crate) fn take_quoted<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
) -> Option<String> {
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
