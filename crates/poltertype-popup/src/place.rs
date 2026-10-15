//! Placement of the tooltip: around its anchor point — the text caret —
//! or at a spot inside the focused window.
//!
//! Around a point it prefers to hang *above* the point, then walks above → below → right
//! → left for the first side that fits fully on screen. Whatever side
//! wins, the transverse coordinate is clamped inside the screen with a
//! small margin, so a point near a corner slides the tooltip along the
//! edge instead of off it.
//!
//! Coordinates are the anchor's space: output-local logical px on
//! Wayland, root pixels on X11.

use crate::enums::WindowSpot;

/// Gap between the point and the tooltip's bottom edge when above.
const GAP_ABOVE: i32 = 18;
/// Gap when below — must also clear the pointer glyph itself.
const GAP_BELOW: i32 = 28;
/// Gap when beside the point (left/right placements).
const GAP_SIDE: i32 = 24;
/// Breathing room kept from every screen edge.
const EDGE_MARGIN: i32 = 8;

/// Top-left position for a `w`×`h` tooltip near the vertical segment
/// `(px, py_top)..(px, py_bottom)` — a caret with its line height, or a
/// pointer where the two coincide.
///
/// `bounds` is the screen size in the same space; `None` degrades to
/// above-else-below with a non-negative clamp, and the compositor clips
/// the rest.
pub(crate) fn place_near_point(
    px: i32,
    py_top: i32,
    py_bottom: i32,
    w: i32,
    h: i32,
    bounds: Option<(i32, i32)>,
) -> (i32, i32) {
    let py_bottom = py_bottom.max(py_top);
    let centered_x = px - w / 2;
    let centered_y = (py_top + py_bottom) / 2 - h / 2;
    let above_y = py_top - GAP_ABOVE - h;

    let Some((bw, bh)) = bounds else {
        let y = if above_y >= 0 {
            above_y
        } else {
            py_bottom + GAP_BELOW
        };
        return (centered_x.max(0), y.max(0));
    };

    let clamp_x = |x: i32| x.clamp(EDGE_MARGIN, (bw - w - EDGE_MARGIN).max(EDGE_MARGIN));
    let clamp_y = |y: i32| y.clamp(EDGE_MARGIN, (bh - h - EDGE_MARGIN).max(EDGE_MARGIN));

    if above_y >= EDGE_MARGIN {
        return (clamp_x(centered_x), above_y);
    }
    let below_y = py_bottom + GAP_BELOW;
    if below_y + h <= bh - EDGE_MARGIN {
        return (clamp_x(centered_x), below_y);
    }
    let right_x = px + GAP_SIDE;
    if right_x + w <= bw - EDGE_MARGIN {
        return (right_x, clamp_y(centered_y));
    }
    let left_x = px - GAP_SIDE - w;
    if left_x >= EDGE_MARGIN {
        return (left_x, clamp_y(centered_y));
    }
    // Nothing fits cleanly (tiny screen / huge tooltip): fall back to
    // the above attempt, clamped fully on-screen.
    (clamp_x(centered_x), clamp_y(above_y))
}

/// Where along one axis of the window a spot sits.
enum Align {
    Start,
    Middle,
    End,
}

fn axes(spot: WindowSpot) -> (Align, Align) {
    use Align::{End, Middle, Start};
    match spot {
        WindowSpot::TopLeft => (Start, Start),
        WindowSpot::TopCenter => (Middle, Start),
        WindowSpot::TopRight => (End, Start),
        WindowSpot::MiddleLeft => (Start, Middle),
        WindowSpot::Center => (Middle, Middle),
        WindowSpot::MiddleRight => (End, Middle),
        WindowSpot::BottomLeft => (Start, End),
        WindowSpot::BottomCenter => (Middle, End),
        WindowSpot::BottomRight => (End, End),
    }
}

fn along(start: i32, len: i32, size: i32, margin: i32, align: &Align) -> i32 {
    match align {
        Align::Start => start + margin,
        Align::Middle => start + (len - size) / 2,
        Align::End => start + len - margin - size,
    }
}

/// Top-left position for a `w`×`h` tooltip at `spot` inside the window
/// `(x, y, width, height)`, held `margin` in from each edge the spot
/// touches. Unclamped — the caller keeps it on its screen, which is the
/// one thing it knows and this does not.
pub(crate) fn place_in_window(
    window: (i32, i32, u32, u32),
    spot: WindowSpot,
    margin: u32,
    w: i32,
    h: i32,
) -> (i32, i32) {
    let (x, y, width, height) = window;
    let (col, row) = axes(spot);
    let margin = i32::try_from(margin).unwrap_or(i32::MAX / 4);
    let (width, height) = (
        i32::try_from(width).unwrap_or(i32::MAX / 4),
        i32::try_from(height).unwrap_or(i32::MAX / 4),
    );
    (
        along(x, width, w, margin, &col),
        along(y, height, h, margin, &row),
    )
}

/// The point inside the window the tooltip will sit at — what picks
/// the screen for a window straddling two. Kept inside the window, so a
/// margin wider than the window cannot pick the screen next to it.
pub(crate) fn window_spot_point(
    window: (i32, i32, u32, u32),
    spot: WindowSpot,
    margin: u32,
) -> (i32, i32) {
    let (x, y, width, height) = window;
    let (px, py) = place_in_window(window, spot, margin, 0, 0);
    let right = x.saturating_add_unsigned(width.saturating_sub(1));
    let bottom = y.saturating_add_unsigned(height.saturating_sub(1));
    (px.clamp(x, right.max(x)), py.clamp(y, bottom.max(y)))
}
