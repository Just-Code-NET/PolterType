//! Point-in-shape tests, all in design units.
//!
//! Every shape is a plain predicate rather than a path — rounded
//! rectangles, a superellipse, discs, a stroked polyline — because "is
//! this point inside?" is all the sampler in `render` ever asks. That
//! keeps the whole icon a few lines of arithmetic: no path flattener,
//! no rasteriser crate.

use super::*;

pub(crate) fn in_round_rect(x: f32, y: f32, rect: &RoundRect) -> bool {
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.w, rect.y + rect.h);
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    // Clamp into the rectangle of corner centres: for any point in the
    // straight part of the edge this lands on the point itself, so the
    // distance test below passes trivially; near a corner it lands on
    // that corner's centre and the test becomes the circle test.
    let cx = x.clamp(x0 + rect.r, x1 - rect.r);
    let cy = y.clamp(y0 + rect.r, y1 - rect.r);
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= rect.r * rect.r
}

pub(crate) fn in_disc(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= r * r
}

/// Even-odd crossing test against a closed outline.
pub(crate) fn in_polygon(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let Some(&last) = poly.last() else {
        return false;
    };
    let mut inside = false;
    let mut prev = last;
    for &(xi, yi) in poly {
        let (xj, yj) = prev;
        // Count the edges a ray cast to -x crosses; an odd count means
        // the point is enclosed. The `(yi > y) != (yj > y)` guard both
        // selects the edges that span the ray and keeps the division
        // below from dividing by zero on horizontal ones.
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        prev = (xi, yi);
    }
    inside
}

/// The ghost silhouette: superellipse dome, vertical sides, notched
/// skirt.
pub(crate) fn in_ghost(x: f32, y: f32) -> bool {
    if y < GHOST_SHOULDER {
        let rx = (GHOST_RIGHT - GHOST_LEFT) / 2.0;
        let ry = GHOST_SHOULDER - GHOST_TOP;
        let dx = (x - (GHOST_LEFT + GHOST_RIGHT) / 2.0) / rx;
        let dy = (y - GHOST_SHOULDER) / ry;
        return dx.abs().powf(GHOST_DOME_EXP) + dy.abs().powf(GHOST_DOME_EXP) <= 1.0;
    }
    if y <= GHOST_HEM_TOP {
        return (GHOST_LEFT..=GHOST_RIGHT).contains(&x);
    }
    in_polygon(x, y, &GHOST_HEM)
}

pub(crate) fn in_eye(x: f32, y: f32) -> bool {
    in_disc(x, y, EYE_LEFT_X, EYE_Y, EYE_R) || in_disc(x, y, EYE_RIGHT_X, EYE_Y, EYE_R)
}

pub(crate) fn in_smile(x: f32, y: f32) -> bool {
    let (x0, y0, x1, y1) = SMILE_BOX;
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    near_polyline(x, y, &SMILE_PATH, SMILE_HALF_STROKE)
}

/// Is the point within `half` of an open polyline? That is exactly a
/// stroke of width `2 * half` with round caps and joins.
pub(crate) fn near_polyline(x: f32, y: f32, path: &[(f32, f32)], half: f32) -> bool {
    path.windows(2).any(|seg| {
        let ((ax, ay), (bx, by)) = (seg[0], seg[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        // Project onto the segment, clamped to its ends — the clamp is
        // what rounds the caps.
        let t = if len2 <= f32::EPSILON {
            0.0
        } else {
            (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0)
        };
        let (ex, ey) = (x - (ax + t * dx), y - (ay + t * dy));
        ex * ex + ey * ey <= half * half
    })
}
