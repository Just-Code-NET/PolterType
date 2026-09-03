//! Plain samples the AT-SPI watchers hand back, and who they belong to.

use std::time::{Duration, Instant};

use crate::focus::CaretHint;

/// One caret-position fix, in coordinates relative to the caret's
/// toplevel window (see [`COORD_TYPE_WINDOW`](super::consts::COORD_TYPE_WINDOW)
/// for why not screen).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaretSample {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) height: u32,
    pub(crate) at: Instant,
    pub(crate) owner: CaretOwner,
}

impl CaretSample {
    /// `age` is computed at read time so the caller can judge staleness
    /// — an old sample usually means the focused app emits no a11y
    /// events at all.
    pub(crate) fn into_hint(self) -> CaretHint {
        CaretHint {
            x: self.x,
            y: self.y,
            height: self.height,
            age: self.at.elapsed(),
            pid: Some(self.owner.pid),
            window: self.owner.window,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FocusSample {
    pub(crate) exe: String,
    pub(crate) at: Instant,
}

impl FocusSample {
    pub(crate) fn age(&self) -> Duration {
        self.at.elapsed()
    }
}

/// The process and window one caret sample came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaretOwner {
    pub(crate) pid: u32,
    /// Size of the toplevel the caret coordinates are relative to, as
    /// the application itself reports it — `None` when it would not
    /// say, leaving the PID as the only identity.
    pub(crate) window: Option<(u32, u32)>,
}
