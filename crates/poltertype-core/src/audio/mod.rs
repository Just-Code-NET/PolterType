//! Sound effects via `rodio`, owned by a dedicated worker thread.
//!
//! Why a worker thread: rodio's `OutputStream` is `!Send` on most
//! platforms because the underlying audio API (CoreAudio, ALSA, …)
//! ties a stream to its creating thread. Wrapping it behind a
//! crossbeam channel keeps `AudioPlayer` `Send + Sync` so the engine
//! can hold an `Arc<AudioPlayer>` on its own thread.
//!
//! Stream lifecycle: we cache **one** `OutputStream` on the worker
//! thread and reuse it across plays. Two reasons for this design vs
//! the obvious "fresh stream per play":
//!
//!   * Per-play `OutputStream::try_default()` costs 20-50 ms on
//!     Windows / macOS and visibly eats the first few milliseconds of
//!     the synth tone — the user hears a clipped, "broken" sound
//!     instead of the intended fade-in.
//!   * The same call also fails intermittently when the OS default
//!     device is mid-switch (BT headphones connecting, HDMI cable
//!     plugged in, …) — leading to silent plays.
//!
//! Default-device tracking is preserved with a *stale refresh*: if
//! the cached stream hasn't been used for [`STREAM_IDLE_REFRESH`],
//! the next play drops it and reopens against the (possibly new)
//! default device. Plus, any play error invalidates the cached
//! stream so the next attempt starts from a clean slate. Together
//! these handle "user just plugged in headphones" gracefully without
//! paying the per-play cost during normal pause / resume bursts.
//!
//! The cached stream is also **released outright** after
//! [`STREAM_IDLE_REFRESH`] with no commands. A permanently open
//! CoreAudio output on an HDMI / DisplayPort device keeps
//! coreaudiod's power assertion alive, which on macOS blocks display
//! and system sleep ("app holds the audio focus" symptom). Letting
//! go of the stream between plays costs one ~20-50 ms reopen, hidden
//! under the synth's lead silence.
//!
//! Themes live in `<config-dir>/sound-themes/<name>/<event>.ogg`.
//! Missing files are silent — we never crash because audio is absent.

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
