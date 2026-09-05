//! The system tray, and the per-OS quirks around it.
//!
//! `tray-icon` is the tray on Windows and macOS and this crate is a
//! pass-through there. On Linux it is not: its `set_tooltip` is an
//! empty function on that platform, so the indicator is built here
//! instead — see [`indicator`] for why that is worth a backend of its
//! own. [`quiet_gtk_tray_logs`] and [`unavailable_reason`] are the
//! noise around it that would otherwise put `#[cfg(target_os)]` in
//! `poltertype-app`.

#![deny(unsafe_op_in_unsafe_fn)]

mod error;
mod icon;
#[cfg(test)]
mod tests;

pub use error::TrayError;
pub use icon::Icon;

#[cfg(target_os = "linux")]
mod indicator;
#[cfg(target_os = "linux")]
pub use indicator::Tray;

#[cfg(not(target_os = "linux"))]
mod upstream;
#[cfg(not(target_os = "linux"))]
pub use upstream::Tray;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{quiet_gtk_tray_logs, unavailable_reason};

#[cfg(not(target_os = "linux"))]
mod noop;
#[cfg(not(target_os = "linux"))]
pub use noop::{quiet_gtk_tray_logs, unavailable_reason};
