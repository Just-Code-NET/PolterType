//! Pure caret-rectangle geometry: degenerate/shape checks, the
//! end-of-text retry offset, and collapsing a glyph rect to an anchor
//! point. No I/O, so this is what `linux_impl/tests.rs` actually
//! exercises without a live a11y bus.

use super::atspi_caret::Extents;

/// A rect that names no glyph. Zero-area is the end-of-text caret
/// position and the answer of objects that cannot measure; the
/// all-`-1` rect is what Chromium and Electron return for a caret
/// offset of `-1` ("no caret here"), and it must not be mistaken for
/// a point one pixel outside the window. (A zero *width* alone is
/// legitimate — zero-advance combining marks.)
pub(super) fn is_degenerate((_, _, width, height): Extents) -> bool {
    width < 0 || height < 0 || (width == 0 && height == 0)
}

/// Whether a rect is narrow enough to *be* a caret rather than the
/// field containing one. A text box's left edge would put the tooltip
/// at the start of the line instead of where the typing is, so only a
/// box no wider than a character or so may stand in for the caret.
pub(super) fn is_caret_shaped((_, _, width, height): Extents) -> bool {
    height > 0 && width >= 0 && width <= 12.max(height.saturating_mul(3) / 2)
}

/// The offset to retry with when the event offset has no glyph: the
/// character *before* the caret, if there is one.
pub(super) fn retry_offset(offset: i32) -> Option<i32> {
    if offset > 0 { Some(offset - 1) } else { None }
}

/// Collapse a glyph rect to the tooltip anchor point. `right_edge`
/// selects the rect's right edge — used when the rect belongs to the
/// character *before* the caret, whose trailing edge is where the
/// caret actually is.
pub(super) fn anchor_from_rect(
    (x, y, width, height): Extents,
    right_edge: bool,
) -> (i32, i32, u32) {
    let anchor_x = if right_edge {
        x.saturating_add(width)
    } else {
        x
    };
    // Toolkits answer sane heights, but the wire type is signed —
    // clamp instead of trusting.
    (anchor_x, y, u32::try_from(height.max(0)).unwrap_or(0))
}
