//! Is the gsettings schema *honoured* on this session, or merely present?
//!
//! Two different ways the schema lies about who owns the keyboard:
//!
//! 1. It ships with GTK, so a bare i3 session with one GTK app has it
//!    installed. There `sources` reads back empty — `try_init` catches
//!    that one by reading the value rather than the exit status.
//! 2. It is populated and looks authoritative, but an input-method
//!    daemon sits between it and the keyboard. On Cinnamon with IBus
//!    mediating input, `gsettings set … current` writes a value
//!    nothing acts on: the layout never changes, no tray indicator
//!    moves — and because we then read our own write back, we believe
//!    it did change and stop correcting entirely
//!    ([#26](https://github.com/Just-Code-NET/PolterType/issues/26)).
//!
//! GNOME is why the schema is probed before IBus in the first place,
//! and why "an IBus daemon is running" cannot be the test: gnome-shell
//! runs IBus too, but it *drives* IBus from this schema, so writing
//! the schema **is** the switch there. The question is therefore not
//! whether IBus is running but whether this shell syncs the schema
//! into it.
//!
//! Fcitx has the same shape and is deliberately not handled here: no
//! one has reported it, and demoting a session on a guess would break
//! the setups where the desktop *does* own the layout and Fcitx only
//! hosts an input method.

/// Desktops whose settings daemon watches
/// `org.gnome.desktop.input-sources` and pushes changes into whatever
/// input method is running. On these the schema stays authoritative
/// even with an IBus daemon alive.
///
/// `XDG_CURRENT_DESKTOP` is a colon-separated *list*, and the GNOME
/// derivatives announce themselves by appending to it — `ubuntu:GNOME`,
/// `Budgie:GNOME`. Splitting is what makes those match; a substring
/// test would also match `X-Cinnamon` variants some distros invent, and
/// an equality test would miss all three.
pub(crate) fn shell_syncs_input_sources(xdg_current_desktop: &str) -> bool {
    xdg_current_desktop.split(':').any(|name| {
        matches!(
            name.trim().to_ascii_uppercase().as_str(),
            "GNOME" | "GNOME-CLASSIC" | "GNOME-FLASHBACK" | "UNITY" | "PANTHEON"
        )
    })
}

/// The rule `try_init` applies once it knows the schema exists and is
/// populated: keep gsettings unless an IBus daemon owns input on a
/// shell that does not feed it from the schema.
pub(crate) fn gsettings_is_authoritative(xdg_current_desktop: &str, ibus_running: bool) -> bool {
    !ibus_running || shell_syncs_input_sources(xdg_current_desktop)
}
