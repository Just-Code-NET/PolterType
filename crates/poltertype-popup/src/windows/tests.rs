#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use super::window::PopupWindow;
use crate::enums::PopupAnchor;
use crate::render::Renderer;
use crate::types::{PopupEntry, PopupModel};

fn model() -> PopupModel {
    PopupModel {
        generation: 1,
        original: "ghbdsn".to_owned(),
        entries: vec![
            PopupEntry {
                text: "привіт".to_owned(),
                badge: Some("UK".to_owned()),
                is_action: false,
            },
            PopupEntry {
                text: "Add to dictionary".to_owned(),
                badge: None,
                is_action: true,
            },
        ],
        accept_hint: Some("Ctrl+Shift".to_owned()),
        timeout: Duration::from_millis(50),
        anchor: PopupAnchor::ScreenBottom { output: None },
    }
}

/// The whole Win32 path end to end without eyes on it: create the
/// layered window, render a real model, hand the surface to
/// `UpdateLayeredWindow`.
///
/// This is what would catch a wrong `BITMAPINFOHEADER`, a DC leaked on
/// an error path, or a premultiplied-alpha mismatch — each of which
/// fails the call rather than merely looking wrong. It does not claim
/// the result is *legible*: only a person can say that.
#[test]
fn the_layered_window_accepts_a_rendered_tooltip() {
    let Some(win) = PopupWindow::create() else {
        // A session with no interactive window station (some CI
        // containers) cannot create a window at all. Skipping beats a
        // red build that says nothing about the code.
        eprintln!("no window station; skipping");
        return;
    };

    let mut renderer = Renderer::new();
    let rendered = renderer.render(&model(), None, 1.0);
    let w = rendered.pixmap.width() as i32;
    let h = rendered.pixmap.height() as i32;
    assert!(w > 0 && h > 0, "renderer produced an empty pixmap");

    assert!(
        win.show(rendered.pixmap.data(), w, h, 0, 0),
        "UpdateLayeredWindow refused a {w}x{h} premultiplied surface"
    );
    win.hide();
}

/// Hovering re-renders, so the second surface has to be accepted just
/// like the first — the path that runs on every mouse move across the
/// tooltip.
#[test]
fn a_second_surface_replaces_the_first() {
    let Some(win) = PopupWindow::create() else {
        eprintln!("no window station; skipping");
        return;
    };
    let mut renderer = Renderer::new();
    let m = model();

    for hover in [None, Some(0), Some(1)] {
        let rendered = renderer.render(&m, hover, 1.0);
        let w = rendered.pixmap.width() as i32;
        let h = rendered.pixmap.height() as i32;
        assert!(
            win.show(rendered.pixmap.data(), w, h, 0, 0),
            "hover={hover:?} surface refused"
        );
    }
    win.hide();
}

/// A zero-sized or short buffer must be refused rather than handed to
/// GDI, which would read past the end of it.
#[test]
fn a_surface_that_does_not_match_its_size_is_refused() {
    let Some(win) = PopupWindow::create() else {
        eprintln!("no window station; skipping");
        return;
    };
    assert!(!win.show(&[0u8; 16], 100, 100, 0, 0), "short buffer");
    assert!(!win.show(&[0u8; 16], 0, 0, 0, 0), "zero size");
}

/// Whatever the monitor reports, the scale has to be a usable
/// multiplier — the renderer refuses a zero or negative one, and a
/// tooltip drawn at 0 would be an empty pixmap.
#[test]
fn the_reported_scale_is_always_positive() {
    let scale = PopupWindow::scale_at(0, 0);
    assert!(scale > 0.0, "scale was {scale}");
}
