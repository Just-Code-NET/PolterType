//! Popup enums: placement anchors and UI events.

/// Where the tooltip should appear.
///
/// No Wayland protocol or X11 property answers "where is the text
/// caret"; the accessibility stack is the one API that does, and only
/// for apps with a live a11y bridge. So the anchors run best-first:
/// the AT-SPI caret when it is available and fresh, the focused
/// window's bottom-centre otherwise (chat inputs and shell prompts
/// live there), a screen edge when nothing is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupAnchor {
    /// A point of interest in global compositor coordinates — the
    /// AT-SPI caret. `height` is the vertical extent at that point
    /// (the caret's line height; 0 when the app reports none):
    /// "above" placements clear the top of it,
    /// "below" placements clear the bottom, so the tooltip never
    /// covers the very line being typed.
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
