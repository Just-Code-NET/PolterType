//! Wayland-friendly keyboard listener via `evdev`, plus `uinput`
//! emitter for replays.
//!
//! ## Listener
//!
//! Open every `/dev/input/event*` device that advertises keyboard
//! capability and read events from all of them on a worker thread.
//!
//! ## Emitter
//!
//! Create a single `uinput` virtual keyboard at start, post Backspace
//! and arbitrary Unicode codepoints to it. Unicode entry on
//! plain-evdev is best-effort: most real GUI apps respect the
//! compose-XKB unicode-input combo (`Ctrl+Shift+U <hex> Enter`),
//! which we drive synthetically.

#![allow(unused_imports, dead_code)] // Linux-only; gated by cfg in lib.rs.

mod consts;
mod devices;
mod emit;
mod emitter;
mod gate;
mod listener;
mod own_nodes;
#[cfg(test)]
mod tests;
mod types;

pub use consts::*;
pub use devices::*;
pub use emit::*;
pub use emitter::*;
pub use gate::*;
pub use listener::*;
pub use types::*;
