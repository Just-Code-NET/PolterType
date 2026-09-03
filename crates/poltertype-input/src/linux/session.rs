//! Which display server this session is talking to.

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
mod tests;
