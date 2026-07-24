//! The focus-tracking extension point.

use super::types::{CaretHint, FocusedWindowGeometry};

/// Best-effort identifier of the currently-focused application.
pub trait FocusTracker: Send + Sync {
    /// File-name of the focused process's executable, e.g.
    /// `"Code.exe"` / `"alacritty"`. Returns `None` if no foreground
    /// window exists, the OS denies the query, or this platform's
    /// implementation is a stub.
    fn focused_exe(&self) -> Option<String>;

    /// Geometry of the focused window, when the backend can answer.
    /// Default `None` — callers must treat geometry as a bonus, never
    /// a given (macOS and GNOME/KDE Wayland have no path to it).
    ///
    /// Not TTL-cached like [`Self::focused_exe`]: it is queried once
    /// per suggestion-tooltip show, not on the per-keystroke path.
    fn focused_window_geometry(&self) -> Option<FocusedWindowGeometry> {
        None
    }

    /// Global pointer position, when the backend can answer. The
    /// suggestion tooltip uses it as a caret proxy when
    /// [`Self::caret_hint`] has nothing fresh: after a click into a
    /// text field the pointer hovers near the caret. Same query
    /// cadence and caching posture as
    /// [`Self::focused_window_geometry`].
    fn pointer_position(&self) -> Option<(i32, i32)> {
        None
    }

    /// Last known on-screen caret position, when a caret source is
    /// running (the AT-SPI watcher on Linux). Same caveats as
    /// [`Self::pointer_position`]: bonus data — many apps expose it,
    /// none guarantee it — queried once per suggestion-tooltip show,
    /// never on the keystroke path. Check [`CaretHint::age`] before
    /// trusting it: a stale sample means the focused app emits no
    /// a11y caret events, and the pointer/window fallback is better.
    fn caret_hint(&self) -> Option<CaretHint> {
        None
    }

    fn backend_name(&self) -> &'static str;
}
