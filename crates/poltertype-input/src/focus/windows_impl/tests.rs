//! The arithmetic between `GetGUIThreadInfo` and the popup's anchor:
//! everything here is pure, so it runs without a foreground window.

use super::*;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// `(x, y, height)` of the hint, or `None`.
fn point_of(hint: Option<CaretHint>) -> Option<(i32, i32, u32)> {
    hint.map(|h| (h.x, h.y, h.height))
}

#[test]
fn a_caret_is_reported_relative_to_the_toplevel_window() {
    // A caret 40×12 into a child control that itself sits at
    // (500, 300) on screen, in a window whose top-left is (100, 80).
    let hint = caret_hint_from(
        POINT { x: 540, y: 312 },
        rect(40, 12, 42, 30),
        rect(100, 80, 900, 700),
    );

    assert_eq!(point_of(hint), Some((440, 232, 18)));
}

#[test]
fn a_collapsed_caret_is_refused() {
    let hint = caret_hint_from(POINT { x: 0, y: 0 }, rect(0, 0, 0, 0), rect(0, 0, 800, 600));

    assert_eq!(point_of(hint), None);
}

#[test]
fn the_sample_needs_no_proof_of_ownership() {
    let hint = caret_hint_from(
        POINT { x: 10, y: 20 },
        rect(0, 0, 2, 16),
        rect(0, 0, 800, 600),
    );

    assert_eq!(
        hint.map(|h| (h.age, h.pid, h.window)),
        Some((Duration::ZERO, None, None))
    );
}

#[test]
fn a_caret_above_or_left_of_the_window_stays_negative() {
    // Anchor resolution rejects a caret outside the focused window; it
    // can only do that if the subtraction does not saturate at zero.
    let hint = caret_hint_from(
        POINT { x: 10, y: 10 },
        rect(0, 0, 2, 16),
        rect(100, 100, 900, 700),
    );

    assert_eq!(point_of(hint), Some((-90, -90, 16)));
}
