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
