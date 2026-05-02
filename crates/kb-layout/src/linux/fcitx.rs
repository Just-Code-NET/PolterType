//! Fcitx5 layout switcher via `fcitx5-remote`.
//!
//! Fcitx5 thinks in terms of *input methods* (IMs), not "layouts".
//! Each XKB layout is exposed as an IM with a name like
//! `keyboard-us` or `keyboard-ua`. The user-facing CLI is
//! `fcitx5-remote`:
//!
//! * `fcitx5-remote -n` → current IM short name.
//! * `fcitx5-remote -s <name>` → switch.
//!
//! Listing all installed IMs from the CLI alone is unreliable across
//! Fcitx5 versions; for v0.1 we expose `current` + `switch_to` and
//! treat `list_active` as "the current one only" — the engine's
//! layout-mapping DB is the source of truth for what we *try* to
//! switch into.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::process::Command;

use tracing::{debug, warn};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

use super::shared::{bcp47_to_xkb, xkb_to_bcp47};

pub struct FcitxSwitcher;

pub fn try_init() -> Option<FcitxSwitcher> {
    // -t 1 = check whether fcitx is running; exits 0 if yes.
    let ok = Command::new("fcitx5-remote")
        .arg("-t")
        .arg("1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    Some(FcitxSwitcher)
}

impl LayoutSwitcher for FcitxSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let raw = run("fcitx5-remote", &["-n"])?;
        let im = raw.trim();
        Ok(LayoutId::new(
            im_to_bcp47(im).unwrap_or_else(|| im.to_owned()),
        ))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        // CLI listing isn't standardised across versions; return
        // the current one only and rely on layout-mapping DB
        // membership for the engine to decide candidates.
        Ok(vec![self.current()?])
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let im = bcp47_to_im(id.as_str());
        let _ = run("fcitx5-remote", &["-s", &im])?;
        debug!(layout = %id, im = %im, "Fcitx5 input method switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-fcitx5-remote"
    }
}

fn im_to_bcp47(im: &str) -> Option<String> {
    let xkb = im.strip_prefix("keyboard-")?;
    Some(xkb_to_bcp47(xkb)?.to_owned())
}

fn bcp47_to_im(bcp: &str) -> String {
    let xkb = bcp47_to_xkb(bcp).unwrap_or("us");
    format!("keyboard-{xkb}")
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
