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
        s if s.contains("english") || s.contains("us") => "us".into(),
        s if s.contains("russian") => "ru".into(),
        s if s.contains("german") => "de".into(),
        s if s.contains("french") => "fr".into(),
        _ => name.to_owned(),
    }
}
