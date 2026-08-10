//! The caret sanity check, which is the one piece of judgement in an
//! otherwise straight-through FFI module — and the piece nobody
//! without a Mac can exercise by hand. The cases are the answers
//! observed on hardware (see the module docs and `MACOS_POPUP.md`).

use core_graphics::geometry::{CGPoint, CGRect, CGSize};

use super::MacosFocusTracker;

fn rect(x: f64, y: f64, w: f64, h: f64) -> CGRect {
    CGRect::new(&CGPoint::new(x, y), &CGSize::new(w, h))
}

/// A text field somewhere on screen, for the "is the caret near its
/// element" half of the check.
fn field() -> CGRect {
    rect(90.0, 195.0, 300.0, 24.0)
}

/// What TextEdit and native fields answer: a sliver one line tall,
/// sitting inside the field.
#[test]
fn a_thin_caret_inside_its_field_is_accepted() {
    assert!(MacosFocusTracker::caret_is_sane(
        rect(100.0, 200.0, 1.0, 16.0),
        Some(field())
    ));
}

/// What Chrome answers: a zero-size rect at the web area's origin. A
/// caret with no height is not a caret.
#[test]
fn a_zero_height_caret_is_rejected() {
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(0.0, 0.0, 0.0, 0.0),
        Some(field())
    ));
}

/// What Terminal answers: a plausibly-shaped sliver, hundreds of
/// points from the element it claims to be in. Accepting it anchors
/// the tooltip where the caret used to be.
#[test]
fn a_caret_far_from_its_element_is_rejected() {
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(100.0, 900.0, 1.0, 16.0),
        Some(field())
    ));
}

/// Real carets stick out of the field's frame by a few points —
/// TextEdit's search field reports one above its own frame — so the
/// neighbourhood check carries slack rather than demanding
/// containment.
#[test]
fn a_caret_just_outside_the_frame_is_still_accepted() {
    assert!(MacosFocusTracker::caret_is_sane(
        rect(100.0, 186.0, 1.0, 8.0),
        Some(field())
    ));
}

/// The other extreme from a zero-size rect: an app answering with the
/// bounds of the whole selection, or of the whole text view.
#[test]
fn a_line_or_block_sized_answer_is_rejected() {
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(100.0, 200.0, 1.0, 400.0),
        Some(field())
    ));
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(100.0, 200.0, 240.0, 16.0),
        Some(field())
    ));
}

/// No frame to compare against (the element exposes no position or
/// size): shape alone has to decide, and a well-shaped caret passes.
#[test]
fn without_an_element_frame_the_shape_alone_decides() {
    assert!(MacosFocusTracker::caret_is_sane(
        rect(100.0, 200.0, 1.0, 16.0),
        None
    ));
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(100.0, 200.0, 1.0, 0.0),
        None
    ));
}

/// AX can hand back a rect built from uninitialised floats; every
/// comparison against a NaN is false, so this is checked explicitly
/// rather than left to the range tests.
#[test]
fn non_finite_answers_are_rejected() {
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(f64::NAN, 200.0, 1.0, 16.0),
        Some(field())
    ));
    assert!(!MacosFocusTracker::caret_is_sane(
        rect(100.0, 200.0, 1.0, f64::INFINITY),
        Some(field())
    ));
}
