//! Where the suggestion tooltip should appear.
//!
//! Pure decision logic — the caller samples the focus tracker, so the
//! chain is unit-testable without a compositor.

use poltertype_input::focus::{CaretHint, FocusedWindowGeometry};
use poltertype_popup::PopupAnchor;
use tracing::debug;

use super::consts::CARET_MAX_AGE;

/// Resolve the anchor from one sample of the focus tracker. Best first:
///
/// 1. **AT-SPI caret** — the real insertion point, when the app exposes
///    it, the sample is fresh, and it lies inside the focused window,
///    so a stale caret from a previous window cannot win.
/// 2. **Focused window** — bottom-centre, the neighbourhood of chat
///    inputs and prompts.
/// 3. **Screen bottom** — nothing known.
///
/// The pointer deliberately no longer sits between the first two: an
/// idle mouse parked mid-screen dragged the tooltip there while the
/// caret sat in a chat box at the bottom edge (`docs/DECISIONS.md`).
/// A wrong anchor is worse than a coarse one.
pub(super) fn resolve_anchor(
    geometry: Option<FocusedWindowGeometry>,
    caret: Option<CaretHint>,
) -> PopupAnchor {
    let Some(g) = geometry else {
        return PopupAnchor::ScreenBottom { output: None };
    };
    match caret_point(caret, &g) {
        Some((x, y, height)) => PopupAnchor::Point {
            x,
            y,
            height,
            output: g.output,
            output_x: g.output_x,
            output_y: g.output_y,
        },
        None => PopupAnchor::WindowRect {
            x: g.x,
            y: g.y,
            width: g.width,
            height: g.height,
            output: g.output,
            output_x: g.output_x,
            output_y: g.output_y,
        },
    }
}

/// The caret's screen position for `g`, or `None` — with a line saying
/// why — when the tooltip has to settle for the window anchor.
///
/// The hint is window-relative; the composed point is checked against
/// the live rect so a nonsense answer from a broken a11y bridge cannot
/// fling the tooltip across the screen. Coordinates only: this path
/// must never log typed text.
fn caret_point(caret: Option<CaretHint>, g: &FocusedWindowGeometry) -> Option<(i32, i32, u32)> {
    let Some(hint) = caret else {
        debug!("no caret sample yet — anchoring the tooltip to the window");
        return None;
    };
    if hint.age > CARET_MAX_AGE {
        debug!(
            age_ms = hint.age.as_millis(),
            "caret sample is stale — anchoring the tooltip to the window"
        );
        return None;
    }
    let (x, y) = (g.x + hint.x, g.y + hint.y);
    if x < g.x || x >= g.x + g.width as i32 || y < g.y || y >= g.y + g.height as i32 {
        debug!("caret sample falls outside the focused window — anchoring to the window");
        return None;
    }
    Some((x, y, hint.height))
}
