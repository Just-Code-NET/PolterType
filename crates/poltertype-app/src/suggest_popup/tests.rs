//! Anchor-chain tests. Pure logic — no compositor, no a11y bus.

use std::time::Duration;

use poltertype_input::focus::{CaretHint, FocusedWindowGeometry};
use poltertype_popup::PopupAnchor;

use super::anchor::resolve_anchor;

/// A window well inside a 2560×1440 output at origin (3488, 560) —
/// the second monitor of the reporter's setup, so the tests exercise
/// non-zero output origins rather than the easy (0, 0) case.
fn window() -> FocusedWindowGeometry {
    FocusedWindowGeometry {
        x: 3540,
        y: 600,
        width: 2400,
        height: 1340,
        output: Some("DP-3".to_owned()),
        output_x: 3488,
        output_y: 560,
    }
}

fn caret(x: i32, y: i32, age: Duration) -> CaretHint {
    CaretHint {
        x,
        y,
        height: 24,
        age,
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
            output: Some("DP-3".to_owned()),
            output_x: 3488,
            output_y: 560,
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
    assert_eq!(anchor, PopupAnchor::ScreenBottom { output: None });
}

/// The regression this module exists for: with no caret available the
/// tooltip lands on the *window*, never wherever the mouse happens to
/// be parked. `resolve_anchor` no longer takes a pointer at all, so
/// the guarantee is now structural — this pins the exact rect it falls
/// back to, whatever the rest of the desktop looks like.
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
            output: g.output,
            output_x: g.output_x,
            output_y: g.output_y,
        }
    );
}
