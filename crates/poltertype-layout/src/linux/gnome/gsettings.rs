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
