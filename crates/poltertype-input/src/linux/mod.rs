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

pub(crate) mod access;
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
/// The **sockets decide**, and `XDG_SESSION_TYPE` only breaks a tie
/// neither of them can. That order is the wrong way round from the
/// obvious one, and it is measured: GDM registers an X11 session while
/// its own greeter is still Wayland, so `XDG_SESSION_TYPE=wayland`
/// reaches the session's every process — logind reports `Type=wayland`
/// for it too. Seen on Ubuntu 26.04 for every X11 session in the
/// desktop matrix (icewm, Xfce, MATE, i3, …), 2026-08-24.
///
/// Believing the variable there costs a real user something: on X11 the
/// X11 backend needs no permissions at all, while the evdev path needs
/// the `input` group — so an X11 user without it was told PolterType
/// could not read their keyboard, on a session where it could have.
///
/// `WAYLAND_DISPLAY` is checked first because under XWayland both
/// sockets exist, and there the compositor owns input, which makes
/// evdev correct.
pub(crate) fn session_kind() -> SessionKind {
    kind_from(
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
    )
}

fn kind_from(wayland_socket: bool, x11_socket: bool, session_type: Option<&str>) -> SessionKind {
    if wayland_socket {
        return SessionKind::Wayland;
    }
    if x11_socket {
        return SessionKind::X11;
    }
    // No socket to go on: a bare-WM setup that sets neither is exactly
    // the crowd the X11 backend exists for, so the variable gets the
    // last word rather than none.
    match session_type {
        Some("x11") => SessionKind::X11,
        Some("wayland") => SessionKind::Wayland,
        _ => SessionKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GDM case, measured on Ubuntu 26.04: an X11 session whose
    /// every process — and logind's own record — says `wayland`,
    /// because the greeter was Wayland when the session was registered.
    /// Only the sockets tell the truth there.
    #[test]
    fn a_lone_x11_socket_outranks_a_variable_saying_wayland() {
        assert_eq!(
            kind_from(false, true, Some("wayland")),
            SessionKind::X11,
            "an X11 session with no Wayland socket is an X11 session"
        );
    }

    #[test]
    fn a_wayland_socket_wins_even_with_xwayland_present() {
        assert_eq!(kind_from(true, true, Some("x11")), SessionKind::Wayland);
        assert_eq!(kind_from(true, false, None), SessionKind::Wayland);
    }

    /// Neither socket: the bare-WM setups the variable exists for.
    #[test]
    fn with_no_socket_the_variable_gets_the_last_word() {
        assert_eq!(kind_from(false, false, Some("x11")), SessionKind::X11);
        assert_eq!(
            kind_from(false, false, Some("wayland")),
            SessionKind::Wayland
        );
        assert_eq!(kind_from(false, false, None), SessionKind::Unknown);
        assert_eq!(kind_from(false, false, Some("tty")), SessionKind::Unknown);
    }
}
