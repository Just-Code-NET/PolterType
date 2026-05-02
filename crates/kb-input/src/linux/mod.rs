//! Linux global keyboard listener (Phase 6 fills these in).
//!
//! Wayland-first. Selection logic at runtime:
//!   1. If `XDG_SESSION_TYPE=x11` → `x11::X11Listener`.
//!   2. If user is in `input` group and `/dev/input/event*` is readable
//!      → `wayland::EvdevListener`.
//!   3. Otherwise fall back to AT-SPI (Phase 6.x) or report Unsupported
//!      so the tray can show an onboarding banner.

use crate::{InputError, InputListener};

mod wayland;
mod x11;

pub fn create_listener() -> Result<Box<dyn InputListener>, InputError> {
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    match session_type.as_str() {
        "x11" => Ok(Box::new(x11::X11Listener::new())),
        "wayland" | "" => Ok(Box::new(wayland::EvdevListener::new())),
        other => Err(InputError::Unsupported(format!(
            "unrecognised XDG_SESSION_TYPE = {other:?}"
        ))),
    }
}
