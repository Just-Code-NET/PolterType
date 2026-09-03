//! Per-OS global keyboard listener: the [`InputListener`] trait, the
//! [`create_listener`] factory that picks a backend at runtime, and
//! [`KeyEvent`] re-exported from `poltertype-types`.
//!
//! `InputListener::start` may spawn its own thread — Windows does,
//! because `WH_KEYBOARD_LL` needs an OS message loop on the installing
//! thread. Events go into a caller-supplied
//! [`crossbeam_channel::Sender`]; the OS hook callback never blocks and
//! never does non-trivial work.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod clipboard;
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

mod chords;
mod consts;
mod enums;
mod factory;
mod gate;
mod hotkey_env;
// The key gate's swallow decision, shared by the Windows and macOS
// gates. Pure std, no OS imports — compiled under `cfg(test)` on every
// host so its safety properties are tested where the project actually
// runs CI.
#[cfg(any(windows, target_os = "macos", test))]
mod hold;
mod traits;
mod types;

pub use clipboard::{Clipboard, ClipboardGap, clipboard, selection_support};
pub use consts::*;
pub use enums::*;
pub use factory::*;
pub use gate::*;
pub use hotkey_env::*;
pub use traits::*;
pub use types::*;
