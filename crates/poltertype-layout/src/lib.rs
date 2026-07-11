//! Per-OS keyboard layout switcher.
//!
//! Public surface:
//! * [`LayoutSwitcher`] — trait every per-OS implementation satisfies.
//! * [`create_switcher`] — runtime factory that picks the right backend.
//!
//! Layout-mapping tables (which key maps to which character per layout)
//! live in `data/layout-mappings/` and are loaded by `poltertype-detect` /
//! `poltertype-core`, not by this crate. We deliberately keep this crate small
//! and OS-focused.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

mod enums;
mod factory;
mod traits;

pub use enums::*;
pub use factory::*;
pub use traits::*;
