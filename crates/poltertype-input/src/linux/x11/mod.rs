//! X11 keyboard listener via XInput2, plus XTest emitter for replays.
//!
//! The listener selects `RawKeyPress` / `RawKeyRelease` on the root
//! window. Raw events arrive regardless of which client holds focus,
//! which is the global-snooping property we need — and unlike evdev it
//! needs **no `input`-group membership and no `sudo`**, so X11 is the
//! one Linux session type where PolterType works straight after
//! `cargo install`.
//!
//! The emitter is `XTestFakeInput`, mirroring the uinput one:
//! [`KeyEmitter::send_keys`] replays scancodes against the freshly
//! locked XKB group, and [`KeyEmitter::send_text`] types arbitrary
//! Unicode by temporarily binding each keysym to a spare keycode (the
//! `xdotool` technique), which smart-commands need.
//!
//! XTest events come back through XInput2 with no synthetic marker —
//! the same trap as uinput-behind-keyd — hence the echo log behind
//! [`KeyEmitter::take_emitted`].

#![allow(unused_imports, dead_code)] // Linux-only; gated by cfg in lib.rs.

mod codes;
mod consts;
mod emit;
mod emitter;
mod events;
mod listener;
mod types;

pub use codes::*;
pub use consts::*;
pub use emit::*;
pub use emitter::*;
pub use events::*;
pub use listener::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
