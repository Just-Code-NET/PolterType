//! Popup enums: placement anchors and UI events.

/// Where the tooltip should appear, best-first: no Wayland protocol or
/// X11 property answers "where is the caret", and the accessibility
/// stack only does so for apps with a live bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupAnchor {
    /// A point of interest in global compositor coordinates — the
    /// AT-SPI caret. `height` is the caret's line height (0 when the
    /// app reports none); "above" placements clear its top and "below"
    /// its bottom, so the tooltip never covers the line being typed.
    Point {
        x: i32,
        y: i32,
        height: u32,
        output: Option<String>,
        output_x: i32,
        output_y: i32,
    },
    /// Geometry of the focused window, in global compositor
    /// coordinates, plus (Wayland only) the name and origin of the
    /// output containing it — layer-shell margins are output-local.
    WindowRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        output: Option<String>,
        output_x: i32,
        output_y: i32,
    },
    /// Nothing known about the focused window — bottom-centre of the
    /// output named (or the compositor's choice when `None`).
    ScreenBottom { output: Option<String> },
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
