//! IBus layout switcher via the `ibus` CLI.
//!
//! IBus (Intelligent Input Bus) hosts input methods that can include
//! plain XKB layouts (`xkb:us::eng`, `xkb:ua::ukr`, …). Switching:
//!
//! * `ibus engine` → current engine name.
//! * `ibus engine <name>` → switch.
//! * `ibus list-engine` → all known engines, formatted as
//!   `language - engine_name` blocks.
//!
//! IBus runs on any DE; we probe for `ibus` in PATH.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::process::Command;

use tracing::{debug, warn};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

use super::shared::{bcp47_to_xkb, xkb_to_bcp47};

pub struct IBusSwitcher;

pub fn try_init() -> Option<IBusSwitcher> {
    // `ibus engine` returns 1 with no message when the daemon is not
    // running; success → we have a usable IBus.
    let ok = Command::new("ibus")
        .arg("engine")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    Some(IBusSwitcher)
}

impl LayoutSwitcher for IBusSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let raw = run("ibus", &["engine"])?;
        let engine = raw.trim();
        Ok(LayoutId::new(
            ibus_engine_to_bcp47(engine).unwrap_or_else(|| engine.to_owned()),
        ))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        let raw = run("ibus", &["list-engine"])?;
        let mut out = Vec::new();
        for line in raw.lines() {
            // Engine names appear as `  xkb:us::eng - ...`.
            let line = line.trim_start();
            if line.starts_with("xkb:") {
                let name = line.split_whitespace().next().unwrap_or("").trim();
                if let Some(bcp) = ibus_engine_to_bcp47(name) {
                    out.push(LayoutId::new(bcp));
                }
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        // Reverse-look the engine name from the BCP-47 tag, falling
        // back to a synthesised `xkb:<short>::<lang>` triplet.
        let engine = bcp47_to_ibus_engine(id.as_str()).unwrap_or_else(|| synth_engine(id.as_str()));
        let _ = run("ibus", &["engine", &engine])?;
        debug!(layout = %id, engine = %engine, "IBus engine switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-ibus-cli"
    }
}

fn ibus_engine_to_bcp47(engine: &str) -> Option<String> {
    // `xkb:us::eng` → ("us", _, "eng") → en-US
    let rest = engine.strip_prefix("xkb:")?;
    let mut parts = rest.split(':');
    let xkb_short = parts.next()?;
    let _variant = parts.next();
    let _iso639 = parts.next();
    Some(xkb_to_bcp47(xkb_short)?.to_owned())
}

fn bcp47_to_ibus_engine(bcp: &str) -> Option<String> {
    let xkb = bcp47_to_xkb(bcp)?;
    Some(synth_engine_from_xkb(xkb, bcp))
}

fn synth_engine(bcp: &str) -> String {
    // "uk-UA" → "xkb:ua::ukr" (best-effort).
    let xkb = bcp47_to_xkb(bcp).unwrap_or("us");
    synth_engine_from_xkb(xkb, bcp)
}

fn synth_engine_from_xkb(xkb: &str, bcp: &str) -> String {
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
