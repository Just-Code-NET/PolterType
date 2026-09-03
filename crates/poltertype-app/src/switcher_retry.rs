//! Building the layout-switcher backend at startup, patiently.

use std::time::Instant;

use poltertype_layout::{LayoutError, create_switcher};
use tracing::debug;

use crate::consts::{SWITCHER_PROBE_INTERVAL, SWITCHER_PROBE_WINDOW};

/// [`create_switcher`], retried for a few seconds before giving up.
///
/// At login we can be started before the session has anything to
/// probe. That is not hypothetical: an `xdg-desktop-autostart` unit
/// beat the Hyprland session's own environment import, PolterType
/// probed seven backends, found none and exited 1 — so "run at login"
/// simply did not work, with the reason in a journal nobody reads.
///
/// Patience costs a genuinely unsupported machine a slower error
/// message, and buys every autostarted one a working app.
pub(crate) fn switcher_with_retry()
-> Result<Box<dyn poltertype_layout::LayoutSwitcher>, LayoutError> {
    let deadline = Instant::now() + SWITCHER_PROBE_WINDOW;
    loop {
        match create_switcher() {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => {
                debug!("no layout switcher backend yet; the session may still be coming up");
                std::thread::sleep(SWITCHER_PROBE_INTERVAL);
            }
        }
    }
}
