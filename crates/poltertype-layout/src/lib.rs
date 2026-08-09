//! Per-OS keyboard layout switcher: the [`LayoutSwitcher`] trait and
//! the [`create_switcher`] factory that picks a backend at runtime.
//!
//! Layout-mapping tables live in `data/layout-mappings/` and are loaded
//! by `poltertype-core`, not by this crate — which stays small and
//! OS-focused.

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
