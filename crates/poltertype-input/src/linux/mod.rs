//! Linux global keyboard listener + emitter.
//!
//! Two backends, picked by session type:
//!
//! 1. **X11** — `XInput2` raw events for the listener, `XTest` for the
//!    emitter. Needs no special permissions at all: any client that can
//!    open the display can select raw events. Nothing to install, no
//!    `sudo`, no group membership.
//! 2. **Wayland** — `evdev` for the listener, `uinput` for the emitter.
//!    Wayland has no global keyboard-snooping protocol by design; the
//!    realistic path is reading `/dev/input/event*` directly. That
//!    requires the user to be in the `input` group + a udev rule —
//!    `scripts/setup-linux.sh` sets both up with one `sudo` prompt. If
//!    permissions aren't granted, the listener returns `InputError::Os`
//!    so the tray can show an onboarding banner.
//!
//! AT-SPI fallback (no `sudo` required, less reliable) lands in v0.1.x.

#![allow(unused_imports, dead_code)] // Linux-only code; Windows doesn't compile this.

use crate::{InputError, InputListener, KeyEmitter};

pub mod wayland;
pub mod x11;

pub fn create_listener() -> Result<Box<dyn InputListener>, InputError> {
    match session_kind() {
        SessionKind::X11 => Ok(Box::new(x11::X11Listener::new())),
        SessionKind::Wayland | SessionKind::Unknown => Ok(Box::new(wayland::EvdevListener::new())),
    }
}

pub fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    match session_kind() {
        SessionKind::X11 => Ok(Box::new(x11::X11Emitter::new())),
        SessionKind::Wayland | SessionKind::Unknown => Ok(Box::new(wayland::UinputEmitter::new())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionKind {
    X11,
    Wayland,
    Unknown,
}

/// Which display server are we talking to?
///
/// `XDG_SESSION_TYPE` answers this when it's set, but plenty of bare-WM
/// setups (i3, openbox, a hand-rolled `.xinitrc`) never set it — and
/// that is precisely the crowd the X11 backend exists for. So when the
/// variable is missing we fall back to the display sockets themselves,
/// checking `WAYLAND_DISPLAY` first: under XWayland *both* it and
/// `DISPLAY` are set, and there the compositor — not the X server —
/// owns input, which makes evdev the correct backend.
pub(crate) fn session_kind() -> SessionKind {
    match std::env::var("XDG_SESSION_TYPE").ok().as_deref() {
        Some("x11") => return SessionKind::X11,
        Some("wayland") => return SessionKind::Wayland,
        _ => {}
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        SessionKind::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        SessionKind::X11
    } else {
        SessionKind::Unknown
    }
}
