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

#[cfg(not(target_os = "linux"))]
mod noop;
#[cfg(not(target_os = "linux"))]
pub use noop::{quiet_gtk_tray_logs, unavailable_reason};
