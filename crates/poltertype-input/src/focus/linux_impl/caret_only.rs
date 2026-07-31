//! Caret-only tracker for Wayland sessions with no active-window query.
//!
//! GNOME and KDE expose no compositor-agnostic "which window has
//! focus" — by design, the same reasoning that keeps global input out
//! of reach. That rules out `focused_exe()` and window geometry, and
//! for a long time the whole tracker was therefore a noop there.
//!
//! But the *caret* comes from AT-SPI, which is a session bus and knows
//! nothing about compositors: it answers on GNOME and KDE exactly as
//! it does on Hyprland. Leaving it unbuilt cost the suggestion tooltip
//! its best anchor on the two largest desktops and pinned it to the
//! bottom of the screen — not for a technical reason, but because the
//! watcher was only ever constructed inside the two branches that had
//! a window query as well.

use std::sync::Arc;

use crate::focus::{CaretHint, FocusTracker};

use super::atspi_caret::{AtspiCaretWatcher, CaretSample};

pub(crate) struct CaretOnlyFocusTracker {
    caret: Arc<AtspiCaretWatcher>,
}

impl CaretOnlyFocusTracker {
    pub(crate) fn new(caret: Arc<AtspiCaretWatcher>) -> Self {
        Self { caret }
    }
}

impl FocusTracker for CaretOnlyFocusTracker {
    /// Always `None`, and deliberately so — everything keyed off the
    /// focused app (`[exceptions].disabled_apps`, per-app wordlist
    /// profiles, `apps = [...]` on smart commands) stays inert here
    /// rather than acting on a guess.
    fn focused_exe(&self) -> Option<String> {
        None
    }

    fn caret_hint(&self) -> Option<CaretHint> {
        self.caret.latest().map(CaretSample::into_hint)
    }

    fn backend_name(&self) -> &'static str {
        "linux-atspi-caret-only"
    }
}
