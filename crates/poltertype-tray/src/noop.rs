//! Tray quirks outside Linux: there are none.

/// No GTK, no GLib log domain to tame.
pub fn quiet_gtk_tray_logs() {}

/// Windows and macOS build the tray against an OS API that is always
/// there; only Linux loads it at runtime and can come up short.
pub fn unavailable_reason() -> Option<String> {
    None
}
