//! `GnomeSwitcher` — layout control via gsettings.

use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct GnomeSwitcher;

pub fn try_init() -> Option<GnomeSwitcher> {
    init(UnreadSchema::StandDown)
}

/// `try_init` without the "this desktop ignores the schema" check —
/// for `POLTERTYPE_LAYOUT_BACKEND=gnome`. The check is a list of
/// desktop names, so it can only ever be as right as the reports
/// behind it; a user whose gsettings switching demonstrably works
/// needs a way to say so that our list cannot override.
pub fn init_without_desktop_check() -> Option<GnomeSwitcher> {
    init(UnreadSchema::Ignore)
}

fn init(unread: UnreadSchema) -> Option<GnomeSwitcher> {
    // Reject if `gsettings` is not in PATH at all.
    let exists = Command::new("gsettings")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !exists {
        return None;
    }
    // Reject if the schema is not installed — `gsettings get` exits
    // non-zero then.
    //
    // Reading the value rather than the exit status is deliberate: the
    // schema ships with GTK, so plenty of machines running no
    // GNOME-family desktop have it. There `sources` reads back empty,
    // and claiming the session on that basis would hand every switch to
    // a backend with nothing to switch between, shadowing the X11/XKB
    // one that would have worked.
    let sources = read_sources().unwrap_or_default();
    if sources.is_empty() {
        return None;
    }
    // Populated is still not the same as read: Cinnamon populates this
    // schema and drives the layout elsewhere (#26). It is probed before
    // this backend and has already had its chance by now — this is the
    // guard for the paths that reach us anyway.
    if matches!(unread, UnreadSchema::StandDown) && crate::linux::cinnamon::session_is_cinnamon() {
        debug!(
            "input-sources schema is populated but Cinnamon does not read it; \
             standing down rather than writing a key nobody acts on"
        );
        return None;
    }
    Some(GnomeSwitcher)
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
        // "gsettings" is the honest backend tag — it's what we shell
        // out to. Picked up by GNOME / Unity / Cinnamon / Budgie /
        // Pantheon / MATE.
        "linux-gsettings"
    }
}
