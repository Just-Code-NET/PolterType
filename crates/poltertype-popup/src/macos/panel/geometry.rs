//! Scale and placement maths — pure functions of the anchor and the
//! live display layout, independent of what is currently shown.

use core_graphics::display::CGDisplay;
use objc2_app_kit::NSScreen;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect};

use crate::enums::PopupAnchor;

/// Popup bottom edge floats this many px above the anchor window's
/// bottom edge (or the screen bottom). Matches the other backends.
const BOTTOM_OFFSET: i32 = 96;

/// Height of the primary screen — the flip reference between the CG
/// and AppKit spaces.
pub(super) fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm)
        .firstObject()
        .map_or(0.0, |s| s.frame().size.height)
}

/// The scale the renderer should draw at for the anchor's screen.
/// Asked per screen via `backingScaleFactor` — the macOS spelling of
/// the Windows per-monitor DPI query.
pub(super) fn scale_at(mtm: MainThreadMarker, anchor: &PopupAnchor) -> f64 {
    let (ax, ay) = anchor_point(anchor);
    let primary_h = primary_height(mtm);
    // Anchor in AppKit coordinates for the frame contains-point test.
    let appkit = NSPoint::new(ax, primary_h - ay);
    for screen in NSScreen::screens(mtm).iter() {
        if point_in_rect(appkit, screen.frame()) {
            return screen.backingScaleFactor();
        }
    }
    1.0
}

/// A point on the display the tooltip is about to appear on (CG
/// space), used to ask that display its scale.
fn anchor_point(anchor: &PopupAnchor) -> (f64, f64) {
    match *anchor {
        PopupAnchor::Point { x, y, .. } => (x as f64, y as f64),
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => (
            (x + width as i32 / 2) as f64,
            (y + height as i32 / 2) as f64,
        ),
        PopupAnchor::ScreenBottom => {
            let (vx, vy, vw, vh) = display_union();
            (vx + vw / 2.0, vy + vh / 2.0)
        }
    }
}

/// The union of every active display's bounds, in CG global
/// coordinates — the same role `GetSystemMetrics(SM_*VIRTUALSCREEN)`
/// plays in the Windows backend.
fn display_union() -> (f64, f64, f64, f64) {
    let Ok(ids) = CGDisplay::active_displays() else {
        let b = CGDisplay::main().bounds();
        return (b.origin.x, b.origin.y, b.size.width, b.size.height);
    };
    let mut union: Option<(f64, f64, f64, f64)> = None;
    for id in ids {
        let b = CGDisplay::new(id).bounds();
        union = Some(match union {
            None => (b.origin.x, b.origin.y, b.size.width, b.size.height),
            Some((x, y, w, h)) => {
                let (x1, y1) = (x.min(b.origin.x), y.min(b.origin.y));
                let (x2, y2) = (
                    (x + w).max(b.origin.x + b.size.width),
                    (y + h).max(b.origin.y + b.size.height),
                );
                (x1, y1, x2 - x1, y2 - y1)
            }
        });
    }
    union.unwrap_or_else(|| {
        let b = CGDisplay::main().bounds();
        (b.origin.x, b.origin.y, b.size.width, b.size.height)
    })
}

/// Placement in CG coordinates: the shared side-picker around the
/// caret for `Point`, centred on the anchor window with the bottom
/// edge `BOTTOM_OFFSET` above its bottom for `WindowRect`; clamped to
/// the display union either way. Mirrors `place` in the Windows
/// backend.
pub(super) fn place(w: f64, h: f64, anchor: &PopupAnchor) -> (f64, f64) {
    let (vx, vy, vw, vh) = display_union();
    let (wi, hi) = (w.ceil() as i32, h.ceil() as i32);
    let (px, py) = match *anchor {
        PopupAnchor::Point { x, y, height, .. } => {
            // `place_near_point` works in a 0-based space; shift the
            // union's origin out and back so a left-hand or upper
            // display (negative coordinates) is handled.
            let (rx, ry) = crate::place::place_near_point(
                x - vx as i32,
                y - vy as i32,
                y - vy as i32 + height as i32,
                wi,
                hi,
                Some((vw as i32, vh as i32)),
            );
            (rx as f64 + vx, ry as f64 + vy)
        }
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => (
            x as f64 + (width as f64 - w) / 2.0,
            y as f64 + height as f64 - BOTTOM_OFFSET as f64 - h,
        ),
        PopupAnchor::ScreenBottom => (vx + (vw - w) / 2.0, vy + vh - BOTTOM_OFFSET as f64 - h),
    };
    (
        px.clamp(vx, (vx + vw - w).max(vx)),
        py.clamp(vy, (vy + vh - h).max(vy)),
    )
}

/// `NSMouseInRect` spelled in Rust — one less AppKit import.
fn point_in_rect(point: NSPoint, rect: NSRect) -> bool {
    point.x >= rect.origin.x
        && point.x < rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y < rect.origin.y + rect.size.height
}
