//! Anchor-chain tests. Pure logic — no compositor, no a11y bus.

use std::time::Duration;

use poltertype_input::focus::{CaretHint, FocusedWindowGeometry};
use poltertype_popup::PopupAnchor;

use super::anchor::resolve_anchor;

/// PID of the window every test focuses.
const FOCUSED_PID: u32 = 1234;

/// A window well inside a 2560×1440 output at origin (3488, 560) — the
/// reporter's second monitor, so the tests exercise a non-zero output
/// origin rather than the easy (0, 0) case.
fn window() -> FocusedWindowGeometry {
    FocusedWindowGeometry {
        x: 3540,
        y: 600,
        width: 2400,
        height: 1340,
        pid: Some(FOCUSED_PID),
    }
}

/// A caret the focused window itself reported.
fn caret(x: i32, y: i32, age: Duration) -> CaretHint {
    CaretHint {
        x,
        y,
        height: 24,
        age,
        pid: Some(FOCUSED_PID),
        window: Some((2400, 1340)),
    }
}

#[test]
fn fresh_caret_inside_the_window_wins() {
    let anchor = resolve_anchor(
        Some(window()),
        Some(caret(187, 1216, Duration::from_millis(40))),
    );
    // Window-relative hint composed with the live window rect.
    assert_eq!(
        anchor,
        PopupAnchor::Point {
            x: 3540 + 187,
            y: 600 + 1216,
            height: 24,
        }
    );
}

#[test]
fn no_caret_falls_back_to_the_window() {
    let anchor = resolve_anchor(Some(window()), None);
    assert!(
        matches!(
            anchor,
            PopupAnchor::WindowRect {
                x: 3540,
                y: 600,
                ..
            }
        ),
        "expected the window rect, got {anchor:?}"
    );
}

#[test]
fn stale_caret_falls_back_to_the_window() {
    let anchor = resolve_anchor(
        Some(window()),
        Some(caret(187, 1216, Duration::from_secs(30))),
    );
    assert!(
        matches!(anchor, PopupAnchor::WindowRect { .. }),
        "a caret from a window the user has left must not win, got {anchor:?}"
    );
}

#[test]
fn caret_outside_the_window_falls_back_to_the_window() {
    // A broken a11y bridge answering in screen coordinates: composing
    // with the window origin doubles it and lands off the window.
    let anchor = resolve_anchor(
        Some(window()),
        Some(caret(3540, 1400, Duration::from_millis(40))),
    );
    assert!(
        matches!(anchor, PopupAnchor::WindowRect { .. }),
        "nonsense extents must not fling the tooltip away, got {anchor:?}"
    );
}

#[test]
fn no_geometry_falls_back_to_the_screen_bottom() {
    let anchor = resolve_anchor(None, Some(caret(187, 1216, Duration::from_millis(40))));
    assert_eq!(anchor, PopupAnchor::ScreenBottom);
}

/// The regression this module exists for: with no caret available the
/// tooltip lands on the *window* rect, never wherever the mouse
/// happens to be parked.
#[test]
fn an_idle_pointer_cannot_drag_the_tooltip_across_the_screen() {
    let g = window();
    assert_eq!(
        resolve_anchor(Some(window()), None),
        PopupAnchor::WindowRect {
            x: g.x,
            y: g.y,
            width: g.width,
            height: g.height,
        }
    );
}

/// The everyday failure: one desktop-wide slot holds whichever
/// application moved a caret last, and the app being typed into —
/// a terminal, an editor that draws its own text — never moves one.
/// The stale sample lands *inside* the focused window, so the bounds
/// check alone cannot see anything wrong with it.
#[test]
fn a_caret_from_another_application_is_refused() {
    let mut other_app = caret(187, 1216, Duration::from_millis(40));
    other_app.pid = Some(FOCUSED_PID + 1);
    let anchor = resolve_anchor(Some(window()), Some(other_app));
    assert!(
        matches!(anchor, PopupAnchor::WindowRect { .. }),
        "another app's caret must not anchor this window's tooltip, got {anchor:?}"
    );
}

/// Same process, different window — an editor or browser with two of
/// them open. The coordinates are relative to whichever window
/// reported them, so the size it reports has to match the one the
/// compositor says is focused.
#[test]
fn a_caret_from_another_window_of_the_same_process_is_refused() {
    let mut other_window = caret(187, 1216, Duration::from_millis(40));
    other_window.window = Some((1200, 1340));
    let anchor = resolve_anchor(Some(window()), Some(other_window));
    assert!(
        matches!(anchor, PopupAnchor::WindowRect { .. }),
        "a caret measured against another window must not win, got {anchor:?}"
    );
}

/// Rounding slack, not a real tolerance: a couple of pixels of
/// disagreement is the same window.
#[test]
fn a_window_size_off_by_rounding_still_matches() {
    let mut rounded = caret(187, 1216, Duration::from_millis(40));
    rounded.window = Some((2398, 1341));
    assert!(matches!(
        resolve_anchor(Some(window()), Some(rounded)),
        PopupAnchor::Point { .. }
    ));
}

/// A backend that identifies neither side — macOS reads the caret off
/// the frontmost window at query time, so there is nothing to prove —
/// must keep anchoring to its caret.
#[test]
fn an_unidentified_caret_and_window_still_pair_up() {
    let mut g = window();
    g.pid = None;
    let mut hint = caret(187, 1216, Duration::from_millis(40));
    hint.pid = None;
    hint.window = None;
    assert!(matches!(
        resolve_anchor(Some(g), Some(hint)),
        PopupAnchor::Point { .. }
    ));
}
