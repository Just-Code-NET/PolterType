//! Pure-logic tests: renderer layout math and hit-testing. No OS
//! connections, so they run headless in CI.
//!
//! `Renderer::new()` scans system fonts; environments with none render
//! empty text runs, so tests that assert on text-driven metrics skip
//! themselves via `has_fonts()`.

use std::time::Duration;

use crate::enums::PopupAnchor;
use crate::render::hit_row;
use crate::renderer::Renderer;
use crate::types::{PopupEntry, PopupModel, RowRect};

fn entry(text: &str, badge: Option<&str>) -> PopupEntry {
    PopupEntry {
        text: text.to_string(),
        badge: badge.map(str::to_string),
        is_action: false,
    }
}

fn model(entries: Vec<PopupEntry>, accept_hint: Option<&str>) -> PopupModel {
    PopupModel {
        generation: 7,
        original: "ghbdtn".to_string(),
        entries,
        accept_hint: accept_hint.map(str::to_string),
        timeout: Duration::from_secs(4),
        anchor: PopupAnchor::ScreenBottom,
    }
}

fn row(index: usize, x: f32, y: f32, w: f32, h: f32) -> RowRect {
    RowRect { index, x, y, w, h }
}

#[test]
fn render_produces_rows_matching_entries() {
    let mut renderer = Renderer::new();
    let m = model(
        vec![
            entry("привіт", None),
            entry("привіт", Some("UK")),
            entry("hello", None),
        ],
        Some("Ctrl+Shift"),
    );
    let rendered = renderer.render(&m, None, 1.0);
    assert_eq!(rendered.rows.len(), 3);
    assert!(rendered.pixmap.width() > 0 && rendered.pixmap.height() > 0);
    // The panel fill alone guarantees non-transparent pixels.
    assert!(rendered.pixmap.data().iter().any(|&b| b != 0));
    // Every hit-box must lie inside the pixmap.
    for r in &rendered.rows {
        assert!(r.x >= 0.0 && r.y >= 0.0);
        assert!(r.x + r.w <= rendered.pixmap.width() as f32);
        assert!(r.y + r.h <= rendered.pixmap.height() as f32);
    }
}

#[test]
fn hit_row_finds_the_right_row() {
    let rows = vec![
        row(0, 12.0, 34.0, 200.0, 30.0),
        row(1, 12.0, 66.0, 200.0, 30.0),
        row(2, 12.0, 98.0, 200.0, 30.0),
    ];
    assert_eq!(hit_row(&rows, 100.0, 80.0), Some(1));
    assert_eq!(hit_row(&rows, 12.0, 34.0), Some(0));
    assert_eq!(hit_row(&rows, 100.0, 110.0), Some(2));
    // Just past the right/bottom edges (exclusive) and in the gap.
    assert_eq!(hit_row(&rows, 212.0, 80.0), None);
    assert_eq!(hit_row(&rows, 100.0, 64.5), None);
    assert_eq!(hit_row(&rows, 5.0, 80.0), None);
    assert_eq!(hit_row(&[], 100.0, 80.0), None);
}

#[test]
fn hit_row_matches_rendered_row_centres() {
    let mut renderer = Renderer::new();
    if !renderer.has_fonts() {
        return;
    }
    let m = model(vec![entry("a", None), entry("b", None)], None);
    let rendered = renderer.render(&m, None, 1.0);
    for (i, r) in rendered.rows.iter().enumerate() {
        assert_eq!(
            hit_row(&rendered.rows, r.x + r.w / 2.0, r.y + r.h / 2.0),
            Some(i)
        );
    }
}

#[test]
fn width_clamps_to_min_and_max() {
    let mut renderer = Renderer::new();
    if !renderer.has_fonts() {
        return;
    }
    let short = renderer.render(&model(vec![entry("ok", None)], None), None, 1.0);
    assert_eq!(short.pixmap.width(), 200, "min width");

    let long = renderer.render(
        &model(
            vec![entry(
                "нескінченно-довжелезне-слово-яке-нізащо-не-влізе-в-панель",
                None,
            )],
            None,
        ),
        None,
        1.0,
    );
    assert_eq!(long.pixmap.width(), 340, "max width");
}

#[test]
fn scale_doubles_device_size() {
    let mut renderer = Renderer::new();
    let m = model(vec![entry("ok", None)], None);
    let s1 = renderer.render(&m, None, 1.0);
    let s2 = renderer.render(&m, None, 2.0);
    assert_eq!(s2.pixmap.width(), s1.pixmap.width() * 2);
    assert_eq!(s2.pixmap.height(), s1.pixmap.height() * 2);
}

#[test]
fn hover_changes_pixels() {
    let mut renderer = Renderer::new();
    let m = model(vec![entry("перше", None), entry("друге", None)], None);
    let plain = renderer.render(&m, None, 1.0);
    let hovered = renderer.render(&m, Some(0), 1.0);
    // Same geometry, different pixels (row highlight + badge tint).
    assert_eq!(plain.pixmap.width(), hovered.pixmap.width());
    assert_eq!(plain.pixmap.height(), hovered.pixmap.height());
    assert_ne!(plain.pixmap.data(), hovered.pixmap.data());
}

#[test]
fn accept_hint_changes_footer() {
    let mut renderer = Renderer::new();
    if !renderer.has_fonts() {
        return;
    }
    // Entries kept short so the footer is the widest line and drives
    // the panel width: the footer is always drawn, so the hint shows
    // up as a wider panel (and different pixels), not a taller one.
    let without = renderer.render(&model(vec![entry("a", None)], None), None, 1.0);
    let with = renderer.render(
        &model(vec![entry("a", None)], Some("Ctrl+Shift")),
        None,
        1.0,
    );
    assert_eq!(without.pixmap.height(), with.pixmap.height());
    assert!(with.pixmap.width() >= without.pixmap.width());
    assert_ne!(without.pixmap.data(), with.pixmap.data());
}

#[test]
fn height_grows_with_entry_count() {
    let mut renderer = Renderer::new();
    let one = renderer.render(&model(vec![entry("a", None)], None), None, 1.0);
    let three = renderer.render(
        &model(
            vec![entry("a", None), entry("b", None), entry("c", None)],
            None,
        ),
        None,
        1.0,
    );
    // Two extra rows: 2 × (30 row + 2 gap) logical px.
    assert_eq!(three.pixmap.height(), one.pixmap.height() + 64);
}

// ─── Point placement (side picking) ──────────────────────────────────

use crate::place::place_near_point;

const W: i32 = 220;
const H: i32 = 220;
const SCREEN: Option<(i32, i32)> = Some((2560, 1440));

#[test]
fn place_prefers_above_the_point() {
    let (x, y) = place_near_point(1280, 700, 700, W, H, SCREEN);
    assert_eq!(x, 1280 - W / 2, "horizontally centred on the point");
    assert!(y + H < 700, "popup sits fully above the point");
}

#[test]
fn place_flips_below_near_the_top_edge() {
    let (_, y) = place_near_point(1280, 60, 60, W, H, SCREEN);
    assert!(y > 60, "no room above → below the point");
    assert!(y + H <= 1440 - 8, "still fully on screen");
}

#[test]
fn place_slides_along_the_edge_near_a_corner() {
    // Top-left corner: below wins vertically, and the horizontal
    // centring is clamped so the popup stays on screen.
    let (x, y) = place_near_point(20, 20, 20, W, H, SCREEN);
    assert!(x >= 8, "clamped off the left edge");
    assert!(y > 20, "below the point");
    // Top-right corner mirrors it.
    let (x, _) = place_near_point(2550, 20, 20, W, H, SCREEN);
    assert!(x + W <= 2560 - 8, "clamped off the right edge");
}

#[test]
fn place_goes_sideways_when_neither_above_nor_below_fits() {
    // A short strip of a screen: the popup is taller than the space
    // above AND below the point → it must go beside the point.
    let bounds = Some((2560, 300));
    let (x, y) = place_near_point(1280, 150, 150, W, H, bounds);
    assert!(
        x >= 1280 + 8 || x + W <= 1280 - 8,
        "beside the point, not over it (x={x})"
    );
    assert!(y >= 8 && y + H <= 300 - 8, "vertically clamped on screen");
    // Same strip, point near the right edge → the left side is the
    // only side with room.
    let (x, _) = place_near_point(2500, 150, 150, W, H, bounds);
    assert!(x + W < 2500, "left of the point");
}

#[test]
fn place_without_bounds_degrades_to_above_or_below() {
    let (x, y) = place_near_point(400, 700, 700, W, H, None);
    assert_eq!((x, y), (400 - W / 2, 700 - 18 - H));
    let (_, y) = place_near_point(400, 30, 30, W, H, None);
    assert!(y > 30, "below when above would be off-screen");
}

#[test]
fn place_respects_the_caret_line_height() {
    // A caret segment 40px tall near the top: the "below" flip must
    // clear the segment's BOTTOM — the line being typed stays
    // uncovered.
    let (_, y) = place_near_point(1280, 60, 100, W, H, SCREEN);
    assert!(
        y >= 100,
        "below placement must clear the caret line, got y={y}"
    );
    // With room above, placement keys off the segment's TOP.
    let (_, y) = place_near_point(1280, 700, 740, W, H, SCREEN);
    assert!(
        y + H <= 700,
        "above placement must clear the caret top, got y={y}"
    );
}
