//! IBus engine-name ↔ BCP-47 mapping and CLI invocation.

use super::*;
use crate::linux::shared::{bcp47_to_xkb, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub(crate) fn ibus_engine_to_bcp47(engine: &str) -> Option<String> {
    // `xkb:us::eng` → ("us", _, "eng") → en-US
    let rest = engine.strip_prefix("xkb:")?;
    let mut parts = rest.split(':');
    let xkb_short = parts.next()?;
    let _variant = parts.next();
    let _iso639 = parts.next();
    Some(xkb_to_bcp47(xkb_short)?.to_owned())
}

pub(crate) fn bcp47_to_ibus_engine(bcp: &str) -> Option<String> {
    let xkb = bcp47_to_xkb(bcp)?;
    Some(synth_engine_from_xkb(xkb, bcp))
}

pub(crate) fn synth_engine(bcp: &str) -> String {
    // "uk-UA" → "xkb:ua::ukr" (best-effort).
    let xkb = bcp47_to_xkb(bcp).unwrap_or("us");
    synth_engine_from_xkb(xkb, bcp)
}

pub(crate) fn synth_engine_from_xkb(xkb: &str, bcp: &str) -> String {
    let iso = match bcp.split('-').next().unwrap_or("en") {
        "en" => "eng",
        "uk" => "ukr",
        "ru" => "rus",
        "de" => "ger",
        "fr" => "fra",
        "es" => "spa",
        "pl" => "pol",
        "el" => "ell",
        l => l,
    };
    format!("xkb:{xkb}::{iso}")
}

pub(crate) fn run(prog: &str, args: &[&str]) -> Result<String, LayoutError> {
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
