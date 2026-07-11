//! `WorkerState` — lazily-(re)opened output stream state.

use super::*;
use rodio::{OutputStream, OutputStreamHandle};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{debug, warn};

/// State owned by the audio worker thread. Not `Send` (because of
/// `OutputStream`); never escapes the worker.
pub(crate) struct WorkerState {
    /// Lazily-opened, long-lived stream. `None` means "not yet opened
    /// or just invalidated; open on next play". `last_opened` is set
    /// when the current `stream` was created — used to time the idle
    /// refresh.
    pub(crate) stream: Option<(OutputStream, OutputStreamHandle)>,
    pub(crate) last_opened: Instant,
    pub(crate) theme_dir: Option<PathBuf>,
    pub(crate) volume: f32,
}

impl WorkerState {
    pub(crate) fn new() -> Self {
        Self {
            stream: None,
            last_opened: Instant::now(),
            theme_dir: None,
            volume: 0.6,
        }
    }

    /// Get a usable handle, opening (or re-opening) the stream as
    /// needed. Returns `None` if the OS refuses to give us a default
    /// device — caller must treat this as "no audio available right
    /// now" and skip the play silently.
    pub(crate) fn handle(&mut self) -> Option<OutputStreamHandle> {
        // Refresh after long idle so default-device changes (BT
        // headphones, HDMI, OS sound-settings change) are eventually
        // picked up. Stream creation is what costs ~20-50 ms; reuse
        // is essentially free.
        if self
            .stream
            .as_ref()
            .is_some_and(|_| self.last_opened.elapsed() > STREAM_IDLE_REFRESH)
        {
            debug!("audio: dropping stale OutputStream after idle refresh window");
            self.stream = None;
        }
        if self.stream.is_none() {
            match OutputStream::try_default() {
                Ok((s, h)) => {
                    self.stream = Some((s, h));
                    self.last_opened = Instant::now();
                }
                Err(e) => {
                    warn!(err = %e, "could not open default audio device; play skipped");
                    return None;
                }
            }
        }
        self.stream.as_ref().map(|(_, h)| h.clone())
    }

    /// Drop the cached stream so the next `handle()` call reopens
    /// from scratch. Used when a play fails: the device may have
    /// gone away (USB unplugged, BT disconnect, OS audio service
    /// restart) and we don't want to keep retrying against a dead
    /// handle.
    pub(crate) fn invalidate(&mut self) {
        self.stream = None;
    }
}
