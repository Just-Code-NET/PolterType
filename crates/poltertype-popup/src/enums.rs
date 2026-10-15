//! Popup enums: placement anchors, the spot inside a window, and UI
//! events.

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
    /// Geometry of the focused window, and where inside it the
    /// tooltip goes: `spot`, held `margin` px in from each edge the
    /// spot touches, in the same units as the rect.
    WindowRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        spot: WindowSpot,
        margin: u32,
    },
    /// Nothing known about the focused window — bottom-centre of
    /// whichever screen the display server considers current.
    ScreenBottom,
}

/// One cell of a 3×3 grid laid over the focused window. The default is
/// what every backend did before the grid existed: centred, hanging
/// above the window's bottom edge — the neighbourhood of chat inputs
/// and shell prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowSpot {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
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
