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
pub mod setup;

#[cfg(target_os = "linux")]
mod linux;
// Compiled under `cfg(test)` on every host, not just macOS: the
// keycode tables inside carry no Apple dependency and are exactly the
// part no Mac-less contributor can otherwise check. See `macos/mod.rs`.
#[cfg(any(target_os = "macos", test))]
mod macos;
// Compiled under `cfg(test)` on every host, not just Windows: the key
// gate's swallow decision carries no Win32 dependency and is the part
// no Windows-less contributor can otherwise check. See `windows/mod.rs`.
#[cfg(any(windows, test))]
mod windows;

mod enums;
mod factory;
mod gate;
// The key gate's swallow decision, shared by the Windows and macOS
// gates. Pure std, no OS imports — compiled under `cfg(test)` on every
// host so its safety properties are tested where the project actually
// runs CI.
#[cfg(any(windows, target_os = "macos", test))]
mod hold;
mod traits;
mod types;

pub use enums::*;
pub use factory::*;
pub use gate::*;
pub use traits::*;
pub use types::*;
