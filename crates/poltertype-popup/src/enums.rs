//! Popup enums: placement anchors and UI events.

/// Where the tooltip should appear.
///
/// There is no caret-position API on any of our Linux paths (and none
/// planned on Wayland), so the anchors are proxies, best first: the
/// pointer position when it sits inside the focused window (the user
/// clicked into the text they're editing — the pointer hovers near
/// the caret), the focused window's bottom-centre otherwise (chat
/// inputs and shell prompts live there), a screen edge when nothing
/// is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupAnchor {
    /// A point of interest in global compositor coordinates — the
    /// caret (AT-SPI) or the pointer standing in for it. `height` is
    /// the vertical extent at that point (the caret's line height; 0
    /// for a pointer): "above" placements clear the top of it,
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
