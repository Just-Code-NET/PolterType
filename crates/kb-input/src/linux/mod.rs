//! Linux global keyboard listener + emitter.
//!
//! Wayland-first. Selection logic at runtime:
//!
//! 1. `XDG_SESSION_TYPE=x11` → `x11::X11Listener` (XInput2, no extra
//!    permissions needed).
//! 2. Otherwise: `wayland::EvdevListener`. Wayland has no global
//!    keyboard-snooping protocol by design; the realistic path is
//!    reading `/dev/input/event*` directly. This requires the user
//!    to be in the `input` group + a udev rule — `scripts/setup-linux.sh`
//!    sets both up with one `sudo` prompt. If permissions aren't
//!    granted, the listener returns `InputError::Os` so the tray can
//!    show an onboarding banner.
//!
//! Emitter:
//!
//! 1. On X11: `XTestFakeKeyEvent`.
//! 2. On Wayland: `uinput` via the same `evdev` crate. Same
//!    permission story as the listener.
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum SessionKind {
    X11,
    Wayland,
    Unknown,
}

pub(crate) fn session_kind() -> SessionKind {
    match std::env::var("XDG_SESSION_TYPE").ok().as_deref() {
        Some("x11") => SessionKind::X11,
        Some("wayland") => SessionKind::Wayland,
        _ => SessionKind::Unknown,
    }
}
