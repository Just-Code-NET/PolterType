//! Placement of the tooltip around its anchor point — the text caret.
//!
//! Prefers to hang *above* the point, then walks above → below → right
//! → left for the first side that fits fully on screen. Whatever side
//! wins, the transverse coordinate is clamped inside the screen with a
//! small margin, so a point near a corner slides the tooltip along the
//! edge instead of off it.
//!
//! Coordinates are the anchor's space: output-local logical px on
//! Wayland, root pixels on X11.

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
