//! fcitx IM-name ↔ BCP-47 mapping and CLI invocation.

use super::*;
use crate::linux::shared::{bcp47_to_xkb, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub(crate) fn im_to_bcp47(im: &str) -> Option<String> {
    let xkb = im.strip_prefix("keyboard-")?;
    Some(xkb_to_bcp47(xkb)?.to_owned())
}

pub(crate) fn bcp47_to_im(bcp: &str) -> String {
    let xkb = bcp47_to_xkb(bcp).unwrap_or("us");
    format!("keyboard-{xkb}")
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
