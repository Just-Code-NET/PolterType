//! How large the desktop draws things: `Xft.dpi` from the root
//! window's `RESOURCE_MANAGER`, over the 96 that means 100%.
//!
//! X11 has no per-monitor scale, and the server's own physical DPI is
//! fiction on most setups. `Xft.dpi` is what the settings daemons of
//! GNOME, Cinnamon, MATE and Xfce write when the user picks a scale,
//! and what GTK and Qt read back — so a tooltip that obeys it
//! grows exactly when the desktop's own text does (issue #79). No
//! resource, no scaling: the tooltip draws as designed, as it always
//! did.

use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

/// The DPI desktops mean by "100%".
const BASE_DPI: f32 = 96.0;
/// Past this the panel outgrows any real screen; a value that large
/// is a typo in someone's `.Xresources`.
const MAX_SCALE: f32 = 4.0;
/// `RESOURCE_MANAGER` is a few KiB on a busy desktop; read it whole.
const MAX_RESOURCE_WORDS: u32 = 64 * 1024;

/// The scale to render at: `Xft.dpi / 96`, never below 1 — a desktop
/// set *smaller* than standard keeps the tooltip at its designed size
/// rather than shrinking text that was sized to be read.
///
/// One `GetProperty` per show, on the popup's own thread; read fresh
/// each time so a scale changed in the desktop's settings applies to
/// the next tooltip without a restart.
pub(super) fn desktop_scale(conn: &RustConnection, root: Window) -> f32 {
    let resources = conn
        .get_property(
            false,
            root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            0,
            MAX_RESOURCE_WORDS,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok());
    let Some(reply) = resources else {
        return 1.0;
    };
    scale_from_resources(&String::from_utf8_lossy(&reply.value))
}

/// The `Xft.dpi` line of an X resource database, as a scale factor.
pub(super) fn scale_from_resources(resources: &str) -> f32 {
    resources
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim() == "Xft.dpi")
        .and_then(|(_, value)| value.trim().parse::<f32>().ok())
        .filter(|dpi| dpi.is_finite() && *dpi > 0.0)
        .map_or(1.0, |dpi| (dpi / BASE_DPI).clamp(1.0, MAX_SCALE))
}
