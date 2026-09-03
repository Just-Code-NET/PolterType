//! Per-OS foreground-app tracking.
//!
//! The engine consults this to decide whether auto-switching suits the
//! focused application — the path that keeps the corrector silent where
//! the user asked for silence (see `docs/DECISIONS.md`).
//!
//! The trait is intentionally minimal: the executable name of the
//! focused window is enough to match against
//! `[exceptions].disabled_apps`.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod windows_impl;

#[cfg(target_os = "linux")]
mod linux_impl;

#[cfg(target_os = "macos")]
mod macos_impl;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod unsupported;

mod factory;
mod noop;
mod traits;
mod types;

pub use factory::*;
pub use noop::*;
pub use traits::*;
pub use types::*;
