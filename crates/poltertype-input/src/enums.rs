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
#[repr(u8)]
pub enum ReplaySpeed {
    /// The measured pacing, safe behind a remapper.
    #[default]
    Normal = 0,
    /// Half of it.
    Fast = 1,
    /// None at all: key events go out as fast as the backend accepts
    /// them.
    Instant = 2,
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

    /// The setting as one byte, and back.
    ///
    /// A running emitter is shared across threads and holds its speed
    /// in an atomic, so the setting can be changed without a restart
    /// (issue #67) — and an enum is not something an atomic can hold.
    /// An unknown byte reads as `Normal`, which is also what a
    /// hand-edited config falls back to.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Fast,
            2 => Self::Instant,
            _ => Self::Normal,
        }
    }

    /// Scale one of the engine's *settle* windows — the waits between
    /// switching the layout and typing against it.
    ///
    /// Unlike [`pace`](Self::pace) this never reaches zero, because
    /// the two wait on different things. A pace waits on a device we
    /// own, and the guards around the boundary key hold the line when
    /// it goes. A settle waits on another process's clock — a
    /// compositor propagating xkb state, a settings daemon that may
    /// put its own group back — and arriving early there does not cost
    /// a keystroke: it retypes the whole word in the layout we just
    /// left, which is the one outcome worse than being slow. So the
    /// fastest gear keeps a quarter, and that quarter is still a floor
    /// measured against a real compositor rather than a guess.
    pub fn settle(self, window: Duration) -> Duration {
        match self {
            Self::Normal => window,
            Self::Fast => window / 2,
            Self::Instant => window / 4,
        }
    }

    /// Scale one of an emitter's measured inter-key pauses.
    ///
    /// Only *pacing* goes through here. The guards around the boundary
    /// key — the release before a press the user is still holding, and
    /// the hold after it — are correctness, not speed, and every
    /// backend keeps them as measured whatever this returns.
    ///
    /// Public for the same reason [`settle`](Self::settle) is, and it
    /// matters more here: only the Linux emitters have a clock pause
    /// to scale — Windows hands a whole burst to one `SendInput` call
    /// and macOS paces against the window server's own echo — so a
    /// crate-private version has no caller at all on those two, and
    /// `-D warnings` reads that as dead code. It failed CI on both for
    /// two releases.
    pub fn pace(self, step: Duration) -> Duration {
        match self {
            Self::Normal => step,
            Self::Fast => step / 2,
            Self::Instant => Duration::ZERO,
        }
    }
}

/// A key the engine can claim for the suggestion tooltip: kept from
/// the focused application for as long as an offer is up, and handed
/// to the engine instead. See [`KeyGate::claim`](crate::KeyGate::claim).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKey {
    Up,
    Down,
    /// Return and keypad Enter alike.
    Enter,
    /// `F1`..=`F9`, by number.
    Function(u8),
    /// The main row's `1`..=`9`, by number — claimed only together with
    /// the accept chord's modifiers, see
    /// [`ClaimSet::with_digit_modifiers`](crate::ClaimSet::with_digit_modifiers).
    Digit(u8),
}

impl ClaimKey {
    /// Which claimable key an engine scancode names. Meaningful only
    /// for an event a listener stamped
    /// [`claimed`](poltertype_types::KeyEvent::claimed): the engine's
    /// two code spaces — SC Set-1 from Windows and macOS, evdev from
    /// Linux — disagree on the arrows and keypad Enter, and SC-1's
    /// arrow codes are evdev's keypad digits, which no listener ever
    /// claims.
    pub fn from_scancode(sc: u32) -> Option<Self> {
        match sc {
            0x48 | 0x67 => Some(Self::Up),
            0x50 | 0x6C => Some(Self::Down),
            0x1C | 0x60 => Some(Self::Enter),
            0x3B..=0x43 => Some(Self::Function((sc - 0x3A) as u8)),
            0x02..=0x0A => Some(Self::Digit((sc - 0x01) as u8)),
            _ => None,
        }
    }

    /// The one bit this key occupies in a [`ClaimSet`](crate::ClaimSet).
    pub(crate) fn bit(self) -> u32 {
        match self {
            Self::Up => 1,
            Self::Down => 1 << 1,
            Self::Enter => 1 << 2,
            Self::Function(n) => 1 << (2 + u32::from(n.clamp(1, 9))),
            Self::Digit(n) => 1 << (11 + u32::from(n.clamp(1, 9))),
        }
    }

    /// Every claimable key, in bit order.
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        [Self::Up, Self::Down, Self::Enter]
            .into_iter()
            .chain((1..=9).map(Self::Function))
            .chain((1..=9).map(Self::Digit))
    }
}

#[cfg(test)]
mod tests;
