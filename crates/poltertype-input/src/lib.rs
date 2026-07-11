//! Per-OS global keyboard listener.
//!
//! Public surface:
//! * [`InputListener`] — trait every per-OS implementation satisfies.
//! * [`create_listener`] — runtime factory that picks the right backend.
//! * [`KeyEvent`] — re-exported from `poltertype-types`.
//!
//! ## Threading model
//!
//! `InputListener::start` may spawn its own dedicated thread (Windows
//! does, because `WH_KEYBOARD_LL` requires an OS message loop on the
//! installing thread). The listener pushes events into a
//! [`crossbeam_channel::Sender`] supplied by the caller — never blocking
//! and never doing any non-trivial work in the OS hook callback.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod focus;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

mod enums;
mod factory;
mod traits;
mod types;

pub use enums::*;
pub use factory::*;
pub use traits::*;
pub use types::*;
