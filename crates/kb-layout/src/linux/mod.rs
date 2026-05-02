//! Linux layout switcher — D-Bus to whichever keyboard daemon owns the
//! session, in priority order:
//!
//! 1. GNOME (`org.gnome.Shell` + GSettings `org.gnome.desktop.input-sources`).
//! 2. KDE Plasma (`org.kde.keyboard.layouts`).
//! 3. IBus (`org.freedesktop.IBus`).
//! 4. Fcitx (`org.fcitx.Fcitx5`).
//! 5. X11 fallback via `XkbLockGroup` (lands in v0.1.x).
//!
//! v0.1 ships only the GNOME backend — it covers the largest
//! fraction of Wayland users; the rest are TODOs that surface a
//! clear `Unsupported` so the user understands what's missing.

#![allow(unused_imports, dead_code)] // Linux-only.

use crate::{LayoutError, LayoutId, LayoutSwitcher};

pub mod gnome;
pub mod kde;
pub mod x11;

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    // Probe each backend in priority order; the first that initialises
    // wins. `gnome::try_init` etc. each do a cheap D-Bus reachability
    // check.
    if let Some(g) = gnome::try_init() {
        return Ok(Box::new(g));
    }
    Err(LayoutError::Unsupported(
        "no supported Linux layout-switching backend reachable; \
         GNOME D-Bus probed and not found. KDE / IBus / Fcitx land in v0.1.x"
            .into(),
    ))
}
