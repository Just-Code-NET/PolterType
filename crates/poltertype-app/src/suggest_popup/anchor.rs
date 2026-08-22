//! Where the suggestion tooltip should appear.
//!
//! Pure decision logic — the caller samples the focus tracker, so the
//! chain is unit-testable without a compositor.

use poltertype_input::focus::{CaretHint, FocusedWindowGeometry};
use poltertype_popup::PopupAnchor;
use tracing::debug;

use super::consts::{CARET_MAX_AGE, WINDOW_SIZE_SLACK};

/// Resolve the anchor from one sample of the focus tracker. Best first:
///
/// 1. **AT-SPI caret** — the real insertion point, when the app exposes
///    it, the sample is fresh, and it provably belongs to the focused
///    window, so a caret from a previous window cannot win.
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
        return PopupAnchor::ScreenBottom;
    };
    match caret_point(caret, &g) {
        Some((x, y, height)) => PopupAnchor::Point { x, y, height },
        None => PopupAnchor::WindowRect {
            x: g.x,
            y: g.y,
            width: g.width,
            height: g.height,
        },
    }
}

/// The caret's screen position for `g`, or `None` — with a line saying
/// why — when the tooltip has to settle for the window anchor.
///
/// The hint is window-relative, and one desktop-wide slot holds
/// whichever application moved a caret last, so ownership is checked
/// before the composition and the composed point against the live rect
/// after it. Coordinates only: this path must never log typed text.
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
    if !same_window(&hint, g) {
        return None;
    }
    let (x, y) = (g.x + hint.x, g.y + hint.y);
    if x < g.x || x >= g.x + g.width as i32 || y < g.y || y >= g.y + g.height as i32 {
        debug!("caret sample falls outside the focused window — anchoring to the window");
        return None;
    }
    Some((x, y, hint.height))
}

/// Whether the sample and the focused window describe the same thing.
///
/// Two independent signals, each checked only when both sides can
/// answer — a backend that reports neither (macOS, which reads the
/// caret off the frontmost window at query time) keeps its sample.
///
/// * the **process**: the caret slot is desktop-wide, and the app the
///   user is typing in is very often one with no caret of its own —
///   a terminal, an editor drawing its own text — so the sample left
///   in it belongs to whatever they were doing before;
/// * the **window size** as the reporting app itself measures it,
///   which separates two windows of the *same* process and catches a
///   toolkit whose coordinates are not in the compositor's units.
fn same_window(hint: &CaretHint, g: &FocusedWindowGeometry) -> bool {
    if let (Some(caret_pid), Some(window_pid)) = (hint.pid, g.pid)
        && caret_pid != window_pid
    {
        debug!(
            caret_pid,
            window_pid, "caret sample belongs to another process — anchoring to the window"
        );
        return false;
    }
    if let Some((w, h)) = hint.window
        && (w.abs_diff(g.width) > WINDOW_SIZE_SLACK || h.abs_diff(g.height) > WINDOW_SIZE_SLACK)
    {
        debug!(
            caret_window = ?(w, h),
            focused_window = ?(g.width, g.height),
            "caret sample measures a different window — anchoring to the window"
        );
        return false;
    }
    true
}
