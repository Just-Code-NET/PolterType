//! Input subsystem errors and the replay pacing.

use crate::*;
pub use focus::{FocusTracker, NoopFocusTracker, create_focus_tracker};
pub use poltertype_types::{KeyDirection, KeyEvent, Modifiers};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("the active platform does not support a global keyboard listener: {0}")]
    Unsupported(String),
    #[error("OS error while installing keyboard hook: {0}")]
    Os(String),
    #[error("listener already started")]
    AlreadyStarted,
}

/// How fast a correction is typed out — `[engine].replay_speed`.
///
/// The pauses inside a replay are not politeness. An input remapper
/// proxying our virtual keyboard — keyd and everything built like it —
/// coalesces or drops press/release pairs that land microseconds
/// apart, and the user loses a letter; on X11 the same holds for a
/// client that reads its keyboard faster than the server sequences our
/// injections. The default is the pacing those were measured against.
/// The faster settings trade it away for a correction that lands
/// sooner, and are the user's call because only they can see whether
/// their input stack drops keys.
///
/// Windows synthesises a whole burst in one `SendInput` call and macOS
/// paces against the window server's own echo, so neither has a clock
/// pause to relax: this is a Linux setting in effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplaySpeed {
    /// The measured pacing, safe behind a remapper.
    #[default]
    Normal,
    /// Half of it.
    Fast,
    /// None at all: key events go out as fast as the backend accepts
    /// them.
    Instant,
}

impl ReplaySpeed {
    /// Parse the `config.toml` value, falling back to `Normal` on
    /// anything unrecognised — the same forgiving posture the rest of
    /// the schema takes towards a hand-edited file.
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "fast" => Self::Fast,
            "instant" => Self::Instant,
            _ => Self::Normal,
        }
    }

    /// The canonical `config.toml` spelling.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Fast => "fast",
            Self::Instant => "instant",
        }
    }

    /// Scale one of an emitter's measured inter-key pauses.
    ///
    /// Only *pacing* goes through here. The guards around the boundary
    /// key — the release before a press the user is still holding, and
    /// the hold after it — are correctness, not speed, and every
    /// backend keeps them as measured whatever this returns.
    pub(crate) fn pace(self, step: Duration) -> Duration {
        match self {
            Self::Normal => step,
            Self::Fast => step / 2,
            Self::Instant => Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests;
