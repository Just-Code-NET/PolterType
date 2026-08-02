//! AT-SPI-only tracker for Wayland sessions with no active-window query.
//!
//! GNOME and KDE expose no compositor-agnostic "which window has
//! focus" — by design, the same reasoning that keeps global input out
//! of reach. That ruled out `focused_exe()` and window geometry, and
//! for a long time the whole tracker was therefore a noop there.
//!
//! Both halves now come from AT-SPI, which is a session-bus service
//! and knows nothing about compositors: it answers on GNOME and KDE
//! exactly as it does on Hyprland.
//!
//! * The **caret** comes from `object:text-caret-moved` extents.
//! * The **focused application** comes from `window:activate`, by
//!   asking the a11y bus which process owns the sending connection —
//!   see [`super::atspi_focus`], which is also where the limits of
//!   that answer are written down.
//!
//! The limit worth repeating here, because this is the type that
//! decides whether `disabled_apps` fires: **an application with no
//! accessibility bridge is invisible to this tracker**, and a
//! terminal usually has none. A stale answer is therefore possible in
//! a way it is not on Hyprland or X11, so focus samples carry an age
//! and anything older than [`FOCUS_MAX_AGE`] is treated as no answer.
//! Reporting the wrong application would silence PolterType in a
//! window the user expects it to work in — the exact regression that
//! made `disabled_apps` empty by default.

use std::sync::Arc;
use std::time::Duration;

use crate::focus::{CaretHint, FocusTracker};

use super::atspi_caret::{AtspiCaretWatcher, CaretSample};
use super::atspi_focus::AtspiFocusWatcher;

/// How long a focus observation stays trustworthy.
///
/// Generous, because the events are sparse: a user can sit in one
/// window for hours and the last `window:activate` is still correct.
/// What this bounds is the other case — the user moved to an app with
/// no a11y bridge, nothing was emitted, and the previous answer is
/// now a lie. Five minutes keeps the common case working while
/// ensuring a wrong answer expires rather than persisting all session.
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
    /// leave the focus-keyed features inert, which is the same
    /// behaviour these sessions had before and is the safe direction
    /// to be wrong in.
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
