//! AT-SPI-only tracker for Wayland sessions with no active-window query.
//!
//! GNOME and KDE expose no compositor-agnostic "which window has
//! focus". Both halves come from AT-SPI, a session-bus service that
//! knows nothing about compositors: the **caret** from
//! `object:text-caret-moved` extents, and the **focused application**
//! from `window:activate`, by asking the bus which process owns the
//! sending connection — see [`super::atspi_focus`] for the limits of
//! that answer.
//!
//! One limit is worth repeating here, because this is the type that
//! decides whether `disabled_apps` fires: **an application with no
//! accessibility bridge is invisible to this tracker**, and a terminal
//! usually has none. A stale answer is possible in a way it is not on
//! Hyprland or X11, so samples carry an age and anything older than
//! [`FOCUS_MAX_AGE`] is treated as no answer. Reporting the wrong
//! application would silence PolterType in a window the user expects it
//! to work in.

use std::sync::Arc;
use std::time::Duration;

use crate::focus::{CaretHint, FocusTracker};

use super::atspi_caret::AtspiCaretWatcher;
use super::atspi_focus::AtspiFocusWatcher;
use super::types::CaretSample;

/// How long a focus observation stays trustworthy.
///
/// Generous, because the events are sparse — a user can sit in one
/// window for hours and the last `window:activate` is still correct.
/// What this bounds is the other case: they moved to an app with no
/// a11y bridge, nothing was emitted, and the previous answer is now a
/// lie.
const FOCUS_MAX_AGE: Duration = Duration::from_secs(300);

pub(crate) struct CaretOnlyFocusTracker {
    caret: Arc<AtspiCaretWatcher>,
    focus: Option<Arc<AtspiFocusWatcher>>,
}

impl CaretOnlyFocusTracker {
    pub(crate) fn new(
        caret: Arc<AtspiCaretWatcher>,
        focus: Option<Arc<AtspiFocusWatcher>>,
    ) -> Self {
        Self { caret, focus }
    }
}

impl FocusTracker for CaretOnlyFocusTracker {
    /// The focused application, when AT-SPI has told us recently.
    ///
    /// `None` whenever the watcher could not start, nothing has been
    /// heard yet, or the last observation has gone stale — all three
    /// leave the focus-keyed features inert, which is the safe
    /// direction to be wrong in.
    fn focused_exe(&self) -> Option<String> {
        let sample = self.focus.as_ref()?.latest()?;
        (sample.age() < FOCUS_MAX_AGE).then_some(sample.exe)
    }

    fn caret_hint(&self) -> Option<CaretHint> {
        self.caret.latest().map(CaretSample::into_hint)
    }

    fn backend_name(&self) -> &'static str {
        "linux-atspi"
    }
}
