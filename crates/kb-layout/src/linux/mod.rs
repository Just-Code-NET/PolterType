//! Linux layout switcher (Phase 6 fills these in).
//!
//! Strategy:
//!   1. D-Bus `org.gnome.desktop.input-sources` (GNOME).
//!   2. D-Bus `org.kde.keyboard.layouts` (KDE).
//!   3. IBus / Fcitx D-Bus interfaces.
//!   4. X11 `XkbLockGroup` for legacy sessions.

use crate::{LayoutError, LayoutId, LayoutSwitcher};

mod gnome;
mod kde;
mod x11;

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    // Phase 6 will probe D-Bus services in priority order. For now we
    // surface the same Unsupported message regardless of session.
    Err(LayoutError::Unsupported(
        "Linux layout switcher not implemented yet (Phase 6); GNOME / KDE / IBus / Fcitx \
         backends will land then"
            .into(),
    ))
}

#[allow(dead_code)]
fn _unused_to_pull_modules(_: &dyn LayoutSwitcher) {
    // Suppresses dead_code on the modules until they are wired up.
}

#[allow(dead_code)]
fn _force_link() -> [&'static str; 3] {
    [gnome::BACKEND_NAME, kde::BACKEND_NAME, x11::BACKEND_NAME]
}
