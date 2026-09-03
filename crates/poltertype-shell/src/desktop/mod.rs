//! Telling a Linux desktop which application a window belongs to.
//!
//! Windows and macOS identify a window by the binary it came from, so
//! [`install_desktop_entry`] and [`window_platform_specific`] are
//! no-ops there. See [`linux`] for what Linux needs and why.

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(not(target_os = "linux"))]
mod other;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(not(target_os = "linux"))]
use other as imp;

pub use imp::{install_desktop_entry, window_platform_specific};
