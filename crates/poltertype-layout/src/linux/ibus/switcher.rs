//! `IBusSwitcher` — layout control via the ibus CLI.

use super::*;
use crate::linux::shared::{bcp47_to_xkb, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

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
