//! Sound effects via `rodio`, owned by a dedicated worker thread.
//!
//! rodio's `OutputStream` is `!Send` on most platforms because the
//! underlying audio API ties a stream to its creating thread. Behind a
//! crossbeam channel `AudioPlayer` stays `Send + Sync`, so the engine
//! can hold an `Arc` of it.
//!
//! **One** `OutputStream` is cached and reused. A fresh stream per play
//! costs 20–50 ms on Windows and macOS, which visibly eats the start of
//! the synth tone, and `try_default()` also fails intermittently while
//! the OS default device is mid-switch.
//!
//! The cached stream is dropped after [`STREAM_IDLE_REFRESH`] of disuse
//! and on any play error. That tracks device changes ("user just plugged
//! in headphones") without paying the reopen during a pause/resume
//! burst, and it releases the device: a permanently open CoreAudio
//! output on HDMI keeps coreaudiod's power assertion alive, which blocks
//! display and system sleep on macOS.
//!
//! Themes live in `<config-dir>/sound-themes/<name>/<event>.ogg`;
//! missing files are silent, never a crash.

mod consts;
mod enums;
mod player;
mod types;
mod worker;

pub(crate) use consts::*;
pub use enums::*;
pub use player::*;
pub(crate) use types::*;
pub(crate) use worker::*;

#[cfg(test)]
mod tests;
