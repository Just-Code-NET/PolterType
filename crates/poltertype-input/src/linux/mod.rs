//! Linux global keyboard listener + emitter, two backends picked by
//! session type:
//!
//! 1. **X11** — `XInput2` raw events, `XTest` for emitting. No special
//!    permissions: any client that can open the display can select raw
//!    events.
//! 2. **Wayland** — `evdev` for listening, `uinput` for emitting.
//!    Wayland has no global keyboard-snooping protocol by design, so
//!    reading `/dev/input/event*` is the realistic path. Needs the
//!    `input` group and a udev rule, both set up by
//!    `scripts/setup-linux.sh`; without them the listener returns
//!    `InputError::Os` and the tray shows an onboarding banner.
//!
//! There is no third backend and there will be no AT-SPI one:
//! `at-spi2-registryd` has no keyboard of its own — on Wayland it
//! relays only what the compositor hands it, and only mutter does
//! (measured on wlroots: `RegisterKeystrokeListener` returns false and
//! no events arrive even with injected keys). See `DECISIONS.md`,
//! 2026-08-01.

#![allow(unused_imports, dead_code)] // Linux-only code; Windows doesn't compile this.

use tracing::info;

use crate::{InputError, InputListener, KeyEmitter};

pub mod portal;
pub mod wayland;
pub mod x11;

pub fn create_listener(gate: &crate::KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    match session_kind() {
        SessionKind::X11 => Ok(Box::new(x11::X11Listener::new())),
        SessionKind::Wayland | SessionKind::Unknown => Ok(Box::new(match gate.evdev_inner() {
            Some(g) => wayland::EvdevListener::with_gate(std::sync::Arc::clone(g)),
            None => wayland::EvdevListener::new(),
        })),
    }
}

/// Only the evdev backend can hold keystrokes back. X11 has its own
/// grab primitives, but the XTest emitter does not race the user the
/// same way — the server serialises injected and real events into one
/// queue.
///
/// Whether the returned gate can hold anything is decided at runtime by
/// [`EvdevGate::probe_availability`], not here. `POLTERTYPE_HOLD_KEYS=0`
/// turns it off outright.
pub fn create_key_gate() -> crate::KeyGate {
    if std::env::var_os("POLTERTYPE_HOLD_KEYS").is_some_and(|v| v == "0") {
        info!("key gate disabled by POLTERTYPE_HOLD_KEYS=0");
        return crate::KeyGate::disabled();
    }
    match session_kind() {
        SessionKind::X11 => crate::KeyGate::disabled(),
        SessionKind::Wayland | SessionKind::Unknown => {
            crate::KeyGate::evdev(std::sync::Arc::new(wayland::EvdevGate::new()))
        }
    }
}

/// Pick the emitter for this session.
///
/// On Wayland `uinput` stays the default: it works on every compositor,
/// needs no consent dialog, and is what the key gate is built around.
/// The portal is tried only when uinput cannot be opened — precisely
/// the "user has not run `setup-linux.sh`" case.
///
/// The order is deliberate. Reversed, it would put a consent dialog in
/// front of every GNOME and KDE user who had already granted the group
/// membership.
pub fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    match session_kind() {
        SessionKind::X11 => Ok(Box::new(x11::X11Emitter::new())),
        SessionKind::Wayland | SessionKind::Unknown => {
            let uinput = wayland::UinputEmitter::new();
            if uinput.is_usable() {
                return Ok(Box::new(uinput));
            }
            if !portal::portal_available() {
                // No portal either: hand back uinput anyway so the
                // failure the user sees is the familiar
                // permissions one the Setup pane explains, not a
                // second story about portals they cannot act on.
                info!("no uinput and no RemoteDesktop portal; staying on uinput for diagnostics");
                return Ok(Box::new(uinput));
            }
            info!("uinput unavailable — trying the RemoteDesktop portal");
            match portal::PortalEmitter::try_new() {
                Ok(p) => Ok(Box::new(p)),
                Err(e) => {
                    info!(%e, "portal emitter unavailable; falling back to uinput");
                    Ok(Box::new(uinput))
                }
            }
        }
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
/// `XDG_SESSION_TYPE` answers when set, but plenty of bare-WM setups
/// never set it — and that is exactly the crowd the X11 backend exists
/// for. So fall back to the display sockets, checking `WAYLAND_DISPLAY`
/// first: under XWayland both are set, and there the compositor owns
/// input, which makes evdev correct.
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
