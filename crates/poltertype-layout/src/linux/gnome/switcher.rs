//! `GnomeSwitcher` — layout control via gsettings.

use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct GnomeSwitcher;

pub fn try_init() -> Option<GnomeSwitcher> {
    init(Mediation::StandDown)
}

/// `try_init` with the mediation check skipped — for
/// `POLTERTYPE_LAYOUT_BACKEND=gnome`. The check is a heuristic over
/// desktop names, so it can be wrong on a shell nobody here has run;
/// a user whose gsettings switching demonstrably works needs a way to
/// say so that our guess cannot override.
pub fn init_even_if_mediated() -> Option<GnomeSwitcher> {
    init(Mediation::Ignore)
}

fn init(mediation: Mediation) -> Option<GnomeSwitcher> {
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
    // Reject if the schema is not installed (most KDE / minimal-DE
    // systems). `gsettings get` exits non-zero if the schema or key
    // is missing.
    //
    // Reading the value rather than just checking the exit status is
    // deliberate: the schema ships with GTK and is therefore installed
    // on plenty of machines that run no GNOME-family desktop at all —
    // a bare i3 or openbox session with one GTK app pulled in. There
    // `sources` reads back as an empty list, and claiming the session
    // on that basis would hand every switch to a backend with nothing
    // to switch *between*, shadowing the X11/XKB backend that would
    // have worked. An empty list means GNOME is not managing input
    // sources here; fall through.
    let sources = read_sources().unwrap_or_default();
    if sources.is_empty() {
        return None;
    }
    // Populated is still not the same as obeyed: an input-method
    // daemon can sit between the schema and the keyboard, and then
    // every `gsettings set` we make is a write nobody reads. See
    // `probe.rs` for why the test is the shell rather than the daemon.
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if matches!(mediation, Mediation::StandDown)
        && !gsettings_is_authoritative(&desktop, crate::linux::ibus::daemon_is_running())
    {
        debug!(
            desktop = %desktop,
            "input-sources schema is populated but IBus mediates input on this desktop; \
             standing down so the IBus backend can claim the session"
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
