//! X11 keyboard listener via XInput2, plus XTest emitter for replays.
//!
//! ## Listener
//!
//! `XInput2` `RawKeyPress` / `RawKeyRelease` selected on the root
//! window. Raw events are delivered regardless of which client holds
//! focus, which is exactly the global-snooping property we need — and
//! unlike the evdev path it needs **no `input`-group membership and no
//! `sudo`**: any client that can open the display can select them.
//! That makes X11 the one Linux session type where poltertype works
//! straight after `cargo install`, with no setup script.
//!
//! ## Emitter
//!
//! `XTestFakeInput`. Two paths, mirroring the uinput emitter:
//! * [`KeyEmitter::send_keys`] replays the original scancodes against
//!   the freshly-locked XKB group — the engine's preferred correction
//!   path on Linux.
//! * [`KeyEmitter::send_text`] types arbitrary Unicode by temporarily
//!   binding each codepoint's keysym to a spare keycode (the
//!   `xdotool` technique). Used by smart-commands, which need to emit
//!   text that no physical key produces.
//!
//! ## Echo suppression
//!
//! XTest events come back to us through XInput2 raw events with no
//! "synthetic" marker — the X server replays them as though a real key
//! moved. This is the same trap as uinput-behind-keyd, so we use the
//! same escape: every emitted key is recorded in an echo log and the
//! engine consumes those off the key stream via
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
