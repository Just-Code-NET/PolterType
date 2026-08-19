//! Per-OS quirks of the system tray.
//!
//! `tray-icon` covers the tray itself everywhere, so this is
//! deliberately not a tray abstraction — the binary still builds its
//! `TrayIcon` directly. What lives here is the platform *noise* around
//! that, which would otherwise put `#[cfg(target_os)]` in
//! `poltertype-app`. Today that is one thing, on Linux — see
//! [`quiet_gtk_tray_logs`].

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{quiet_gtk_tray_logs, unavailable_reason};

/// No GTK, no GLib log domain to tame.
#[cfg(not(target_os = "linux"))]
pub fn quiet_gtk_tray_logs() {}

/// Windows and macOS build the tray against an OS API that is always
/// there; only Linux loads it at runtime and can come up short.
#[cfg(not(target_os = "linux"))]
pub fn unavailable_reason() -> Option<String> {
    None
}
