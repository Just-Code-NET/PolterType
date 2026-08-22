//! Popup enums: placement anchors and UI events.

/// Where the tooltip should appear, best-first: no Wayland protocol or
/// X11 property answers "where is the caret", and the accessibility
/// stack only does so for apps with a live bridge.
///
/// Every coordinate is in the display server's global space. Backends
/// that need something else — layer-shell margins are output-local —
/// derive it from their own view of the outputs, which is the one that
/// agrees with where they are about to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupAnchor {
    /// A point of interest — the AT-SPI caret. `height` is the caret's
    /// line height (0 when the app reports none); "above" placements
    /// clear its top and "below" its bottom, so the tooltip never
    /// covers the line being typed.
    Point { x: i32, y: i32, height: u32 },
    /// Geometry of the focused window.
    WindowRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// Nothing known about the focused window — bottom-centre of
    /// whichever screen the display server considers current.
    ScreenBottom,
}

/// What the user did with the tooltip. Sent to the app over the
/// channel passed to [`crate::create_popup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupUiEvent {
    /// An entry was clicked.
    Accepted { generation: u64, index: usize },
    /// The tooltip hid itself after its timeout elapsed.
    TimedOut { generation: u64 },
}
