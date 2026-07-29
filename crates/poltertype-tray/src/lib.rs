//! Per-OS quirks of the system tray.
//!
//! `tray-icon` covers the tray itself on all three platforms, so this
//! crate is deliberately not a tray abstraction — the binary still
//! builds its `TrayIcon` directly. What lives here is the platform
//! *noise* around that: things one OS's tray stack does that the
//! others don't, and that would otherwise put `#[cfg(target_os)]` in
//! `poltertype-app`, which by project rule holds none.
//!
//! Today that is exactly one thing, on Linux — see [`quiet_gtk_tray_logs`].

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::quiet_gtk_tray_logs;

/// No GTK, no GLib log domain to tame.
#[cfg(not(target_os = "linux"))]
pub fn quiet_gtk_tray_logs() {}
