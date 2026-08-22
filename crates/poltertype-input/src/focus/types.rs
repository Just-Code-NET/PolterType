//! Plain data returned by the focus tracker.

/// Geometry of the focused window, in the compositor's global logical
/// coordinates. Used by the suggestion tooltip to appear near where
/// the user is typing — the anchor of last resort when no
/// [`CaretHint`] is available (apps without a11y support).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedWindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Process owning the window, when the backend knows it. Half of
    /// the proof that a [`CaretHint`] describes *this* window rather
    /// than whichever app last moved a caret anywhere on the desktop.
    pub pid: Option<u32>,
}

/// A recent caret position, in coordinates **relative to the caret's
/// toplevel window** — compose with [`FocusedWindowGeometry`] for the
/// screen position. Window-relative on purpose: native-Wayland
/// toolkits report screen coordinates against the window's *initial*
/// placement, which goes stale on every re-tile.
///
/// Produced by the AT-SPI watcher on Linux. `height` is the caret's
/// line height and may legitimately be 0, so never divide by it. `age`
/// is how long ago the underlying event fired — samples stop updating
/// the moment focus lands in an app without a11y support, which is
/// exactly why [`Self::pid`] and [`Self::window`] exist: the last
/// sample seen is then someone else's, and composing it with the
/// focused window's rect puts the tooltip anywhere at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretHint {
    pub x: i32,
    pub y: i32,
    pub height: u32,
    pub age: std::time::Duration,
    /// Process that reported this caret. `None` where the platform
    /// has no notion of one — macOS queries the frontmost application
    /// directly, so the answer is the focused process by construction.
    pub pid: Option<u32>,
    /// Size of the window `x`/`y` are measured against, as that window
    /// reports it. `None` when the backend could not ask.
    pub window: Option<(u32, u32)>,
}
