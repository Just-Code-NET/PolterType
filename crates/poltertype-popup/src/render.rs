//! Drawing primitives shared by [`crate::renderer::Renderer`] and the
//! row hit-test every backend needs for pointer handling.

use tiny_skia::{
    Color, FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Shader, Stroke, Transform,
};

use crate::consts::Rgba;
use crate::types::{PopupModel, RowRect};

/// First row rect containing the point, if any.
pub(crate) fn hit_row(rows: &[RowRect], x: f32, y: f32) -> Option<usize> {
    rows.iter()
        .find(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
        .map(|r| r.index)
}

pub(crate) fn footer_text(model: &PopupModel) -> String {
    match &model.accept_hint {
        Some(hint) => format!("{hint}+1\u{2026}{} · click to replace", model.entries.len()),
        None => "click to replace".to_string(),
    }
}

/// Top edge that vertically centers a `font_size`-tall line (with its
/// standard line height) inside a box starting at `box_y`.
pub(crate) fn centered_top(box_y: f32, box_h: f32, font_size: f32) -> f32 {
    box_y + (box_h - font_size * crate::consts::LINE_HEIGHT_FACTOR) / 2.0
}

/// Alpha-blend an 8-bit coverage mask into the premultiplied-RGBA
/// pixmap using the standard `src + dst × (1 − src_a)` over operator.
pub(crate) fn blit_mask(
    pixmap: &mut Pixmap,
    origin: (i32, i32),
    size: (u32, u32),
    mask: &[u8],
    color: Rgba,
) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let (w, h) = size;
    let data = pixmap.data_mut();
    for row in 0..h as i32 {
        let y = origin.1 + row;
        if y < 0 || y >= ph {
            continue;
        }
        for col in 0..w as i32 {
            let x = origin.0 + col;
            if x < 0 || x >= pw {
                continue;
            }
            let Some(&m) = mask.get((row * w as i32 + col) as usize) else {
                continue;
            };
            if m == 0 {
                continue;
            }
            let a = mul_u8(m, color.3);
            // Premultiply the text color by its effective alpha.
            let (sr, sg, sb) = (mul_u8(color.0, a), mul_u8(color.1, a), mul_u8(color.2, a));
            let idx = ((y * pw + x) * 4) as usize;
            if let Some(px) = data.get_mut(idx..idx + 4) {
                let inv = 255 - a;
                px[0] = sr.saturating_add(mul_u8(px[0], inv));
                px[1] = sg.saturating_add(mul_u8(px[1], inv));
                px[2] = sb.saturating_add(mul_u8(px[2], inv));
                px[3] = a.saturating_add(mul_u8(px[3], inv));
            }
        }
    }
}

fn mul_u8(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}

fn paint(color: Rgba) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(Color::from_rgba8(color.0, color.1, color.2, color.3)),
        anti_alias: true,
        ..Paint::default()
    }
}

pub(crate) fn fill(pixmap: &mut Pixmap, path: &Path, color: Rgba) {
    pixmap.fill_path(
        path,
        &paint(color),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

pub(crate) fn stroke(pixmap: &mut Pixmap, path: &Path, color: Rgba, width: f32) {
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint(color), &stroke, Transform::identity(), None);
}

pub(crate) fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
    let Some(rect) = Rect::from_xywh(x, y, w, h) else {
        return;
    };
    pixmap.fill_rect(rect, &paint(color), Transform::identity(), None);
}

/// Rounded rectangle via cubic quarter-arcs (the classic 0.5523 kappa).
pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.552_284_8 * r;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
