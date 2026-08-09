//! Audio error / event / command enums.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("could not initialise the audio output: {0}")]
    Init(String),
}

#[derive(Debug, Clone, Copy)]
pub enum SoundEvent {
    Correct,
    Pause,
    Resume,
}

impl SoundEvent {
    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::Correct => "correct.ogg",
            Self::Pause => "pause.ogg",
            Self::Resume => "resume.ogg",
        }
    }

    /// Synthesised fallback tone, `(frequency_Hz, duration_ms)`, used
    /// when the user's theme ships no matching `.ogg`. Generating tones
    /// at runtime keeps the binary small and avoids per-platform decoder
    /// quirks; distinct pitches per event give audible feedback without
    /// looking at the tray.
    pub(crate) fn synth_tone(self) -> (f32, u64) {
        match self {
            // Bright "ping" — a correction was applied.
            Self::Correct => (880.0, 90),
            // Lower, longer — auto-switching went off.
            Self::Pause => (440.0, 140),
            // Mid pitch, short — auto-switching is back on.
            Self::Resume => (660.0, 90),
        }
    }
}

#[derive(Debug)]
pub(crate) enum AudioCmd {
    Play(SoundEvent),
    Refresh {
        theme_dir: Option<PathBuf>,
        volume: f32,
    },
    Shutdown,
}
