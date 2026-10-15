//! Plain data structs: the model crossing the app ↔ popup boundary,
//! plus the renderer's own output shapes.

use std::time::Duration;

use tiny_skia::Pixmap;

use crate::enums::PopupAnchor;

/// One clickable row of the tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupEntry {
    /// Replacement text, exactly as it would be typed — or the label
    /// of an action row.
    pub text: String,
    /// Short badge for entries that also switch the keyboard layout
    /// (e.g. `"UK"`) — rendered right-aligned and muted. `None` for
    /// plain spelling suggestions.
    pub badge: Option<String>,
    /// An action row ("Add to dictionary") rather than a replacement:
    /// rendered set-apart — divider above, accent-coloured label.
    pub is_action: bool,
}

/// Everything the popup needs to show one offer.
#[derive(Debug, Clone)]
pub struct PopupModel {
    /// Engine generation stamp; echoed back in
    /// [`crate::PopupUiEvent`] so stale interactions are ignorable.
    pub generation: u64,
    /// The mistyped word, shown struck-through in the header.
    pub original: String,
    /// Rows, in rank order. The engine caps these at 9 (digit keys).
    pub entries: Vec<PopupEntry>,
    /// Footer hint for the accept chord (e.g. `"Ctrl+Shift"`).
    /// `None` = keyboard accept disabled → click-only footer.
    pub accept_hint: Option<String>,
    /// How long to stay on screen before self-hiding with
    /// [`crate::PopupUiEvent::TimedOut`].
    pub timeout: Duration,
    /// Where to appear.
    pub anchor: PopupAnchor,
    /// Size multiplier on top of whatever scale the display reports:
    /// `1.0` draws the tooltip as designed, `2.0` twice as large —
    /// text, padding and panel alike. Out-of-range values are clamped
    /// by the renderer.
    pub scale: f32,
    /// `F1`..`F9` accept instead of `accept_hint` + digit.
    pub function_keys: bool,
    /// `Down` / `Up` / `Enter` navigate — said in the footer.
    pub arrows: bool,
    /// The row the keyboard highlighted. Drawn like a hovered row; the
    /// app re-shows the model to move it.
    pub selected: Option<usize>,
}

/// One clickable row, in pixmap (device-pixel) coordinates.
pub(crate) struct RowRect {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Finished frame: pixels to upload plus where the rows landed.
pub(crate) struct RenderedPopup {
    /// Premultiplied-alpha RGBA, final device pixels.
    pub pixmap: Pixmap,
    pub rows: Vec<RowRect>,
}
