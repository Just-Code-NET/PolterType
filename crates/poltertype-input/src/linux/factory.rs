//! Per-session backend selection: X11 or evdev/Wayland.

use std::sync::Arc;

use tracing::info;

use crate::{InputError, InputListener, KeyEmitter, KeyGate};

use super::session::{SessionKind, session_kind};
use super::{portal, wayland, x11};

pub(crate) fn create_listener(gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    match session_kind() {
        SessionKind::X11 => Ok(Box::new(x11::X11Listener::new())),
        SessionKind::Wayland | SessionKind::Unknown => Ok(Box::new(match gate.backend() {
            Some(g) => wayland::EvdevListener::with_gate(Arc::clone(g)),
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
/// [`EvdevGate::probe_availability`](super::wayland::EvdevGate::probe_availability),
/// not here. `POLTERTYPE_HOLD_KEYS=0` turns it off outright.
pub(crate) fn create_key_gate(_hold_keys: bool) -> KeyGate {
    if std::env::var_os("POLTERTYPE_HOLD_KEYS").is_some_and(|v| v == "0") {
        info!("key gate disabled by POLTERTYPE_HOLD_KEYS=0");
        return KeyGate::disabled();
    }
    match session_kind() {
        SessionKind::X11 => KeyGate::disabled(),
        SessionKind::Wayland | SessionKind::Unknown => {
            KeyGate::with_backend(Arc::new(wayland::EvdevGate::new()))
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
pub(crate) fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
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
