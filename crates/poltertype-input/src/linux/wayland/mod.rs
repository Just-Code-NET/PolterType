//! Wayland-friendly keyboard listener via `evdev`, plus a `uinput`
//! emitter for replays.
//!
//! The listener opens every `/dev/input/event*` that advertises
//! keyboard capability and reads them all on a worker thread. The
//! emitter creates one `uinput` virtual keyboard at start and posts
//! Backspace and Unicode codepoints to it — Unicode entry on plain
//! evdev is best-effort, driving the compose-XKB combo synthetically.

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
