//! Linux layout switcher — shells out to whichever backend the
//! current session uses, in priority order:
//!
//! 1. **Hyprland** (`hyprctl switchxkblayout`) — picked when the
//!    `HYPRLAND_INSTANCE_SIGNATURE` env var is set; the user's
//!    Hyprland config may have `kb_layout = us,ua,…` and we cycle by
//!    index.
//! 2. **KDE Plasma** (`qdbus6` / `qdbus` → `org.kde.keyboard`).
//! 3. **GSettings** (`gsettings org.gnome.desktop.input-sources`) —
//!    covers GNOME, Ubuntu Unity 7+, Cinnamon, Budgie, Pantheon
//!    (elementary OS), MATE. `try_init()` here only matches when the
//!    schema is actually installed, so KDE / standalone-Hyprland
//!    sessions fall through.
//! 4. **IBus** (`ibus engine`) — any DE that hosts IBus.
//! 5. **Fcitx5** (`fcitx5-remote`) — any DE that hosts Fcitx.
//!
//! Each backend's `try_init()` does a cheap reachability probe (env
//! var, schema check, or daemon ping). The first that initialises
//! wins. All backends interact with their daemon via the canonical
//! CLI tool shipped with that ecosystem — that's more robust against
//! D-Bus interface drift between distro / DE versions than raw D-Bus
//! calls (and lets us skip the zbus + async-runtime dep entirely).

#![allow(unused_imports, dead_code)] // Linux-only.

use crate::{LayoutError, LayoutId, LayoutSwitcher};

pub mod fcitx;
pub mod gnome;
pub mod hyprland;
pub mod ibus;
pub mod kde;
pub mod shared;
pub mod x11;

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    let mut tried: Vec<&'static str> = Vec::new();

    if let Some(s) = hyprland::try_init() {
        return Ok(Box::new(s));
    }
    tried.push("hyprland");

    if let Some(s) = kde::try_init() {
        return Ok(Box::new(s));
    }
    tried.push("kde");

    if let Some(s) = gnome::try_init() {
        return Ok(Box::new(s));
    }
    tried.push("gnome");

    if let Some(s) = ibus::try_init() {
        return Ok(Box::new(s));
    }
    tried.push("ibus");

    if let Some(s) = fcitx::try_init() {
        return Ok(Box::new(s));
    }
    tried.push("fcitx");

    Err(LayoutError::Unsupported(format!(
        "no Linux layout-switching backend available; probed: {tried:?}. \
         X11 XkbLockGroup fallback lands in v0.1.x"
    )))
}
