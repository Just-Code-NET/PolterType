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
    /// Name of the output/monitor containing the window (`"eDP-1"`),
    /// when the backend knows it — Wayland layer-shell placement is
    /// per-output. `None` on X11/Windows, where global coordinates
    /// suffice.
    pub output: Option<String>,
    /// Origin of that output in the same global space — converts
    /// window coordinates to output-local ones.
    pub output_x: i32,
    pub output_y: i32,
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
/// the moment focus lands in an app without a11y support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretHint {
    pub x: i32,
    pub y: i32,
    pub height: u32,
    pub age: std::time::Duration,
}
