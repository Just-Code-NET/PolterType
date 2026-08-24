//! `FcitxSwitcher` — layout control via fcitx5-remote.

use super::*;
use crate::linux::shared::{bcp47_to_xkb, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct FcitxSwitcher;

pub fn try_init() -> Option<FcitxSwitcher> {
    // Chosen, not merely running. Ubuntu starts fcitx5 with language
    // support on desktops that route input nowhere near it, and this
    // backend sits ahead of the X11 one in the probe — so "is it up"
    // handed eleven sessions to a switcher that owned nothing. See
    // `shared::session_uses_input_method`.
    if !crate::linux::shared::session_uses_input_method("fcitx") {
        debug!("fcitx5 is not this session's input method; standing down");
        return None;
    }
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
