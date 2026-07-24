//! Shared CPU renderer: turns a [`PopupModel`] into a premultiplied
//! RGBA pixmap plus clickable row hit-boxes. Pure — no OS handles —
//! so both Linux backends reuse it and tests run headless.
//!
//! Layout is computed in *logical* pixels and every panel dimension is
//! rounded to a whole logical pixel before scaling, so the device-pixel
//! buffer is always an exact multiple of the (integer) output scale —
//! Wayland requires buffer size = surface size × buffer scale.

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};
use tiny_skia::{
    Color, FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Shader, Stroke, Transform,
};

use crate::types::PopupModel;

// Logical layout (multiplied by the device scale at draw time).
const MAX_W: f32 = 340.0;
const MIN_W: f32 = 200.0;
const PAD: f32 = 12.0;
const ROW_H: f32 = 30.0;
const ROW_GAP: f32 = 2.0;
const HEADER_H: f32 = 22.0;
const FOOTER_H: f32 = 18.0;
const PANEL_RADIUS: f32 = 10.0;
const BADGE_SIZE: f32 = 18.0;
const BADGE_RADIUS: f32 = 5.0;
const BADGE_GAP: f32 = 10.0;
const HOVER_RADIUS: f32 = 6.0;
const TAG_RADIUS: f32 = 4.0;
const TAG_PAD_X: f32 = 4.0;
const TAG_PAD_Y: f32 = 2.0;
const TAG_GAP: f32 = 8.0;

const HEADER_FONT: f32 = 13.0;
const ROW_FONT: f32 = 15.0;
const BADGE_FONT: f32 = 12.0;
const TAG_FONT: f32 = 11.0;
const FOOTER_FONT: f32 = 11.0;
// Comfortable single-line box; boxes are centered per element anyway.
const LINE_HEIGHT_FACTOR: f32 = 1.2;

type Rgba = (u8, u8, u8, u8);

const PANEL_BG: Rgba = (0x16, 0x16, 0x1E, 0xF2);
const PANEL_BORDER: Rgba = (0xFF, 0xFF, 0xFF, 0x24);
const HEADER_FG: Rgba = (0x9A, 0x9A, 0xB0, 0xFF);
const ROW_FG: Rgba = (0xEC, 0xEC, 0xF4, 0xFF);
/// Action rows ("Add to dictionary") — brand accent, set apart from
/// the plain replacement rows.
const ACTION_FG: Rgba = (0xA7, 0x8B, 0xFA, 0xFF);
/// Hairline divider drawn above the first action row.
const DIVIDER: Rgba = (0xFF, 0xFF, 0xFF, 0x1A);
const BADGE_BG: Rgba = (0x8B, 0x5C, 0xF6, 0xFF);
const BADGE_BG_HOVER: Rgba = (0xA7, 0x8B, 0xFA, 0xFF);
const BADGE_FG: Rgba = (0xFF, 0xFF, 0xFF, 0xFF);
const TAG_FG: Rgba = (0x8B, 0x8B, 0x9E, 0xFF);
const TAG_BG: Rgba = (0xFF, 0xFF, 0xFF, 0x14);
const HOVER_BG: Rgba = (0xFF, 0xFF, 0xFF, 0x12);
const FOOTER_FG: Rgba = (0x70, 0x70, 0x8A, 0xFF);

const ELLIPSIS: char = '…';

/// One clickable row, in pixmap (device-pixel) coordinates.
pub(crate) struct RowRect {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Finished frame: pixels to upload plus where the rows landed.
pub(crate) struct RenderedPopup {
    /// Premultiplied-alpha RGBA, final device pixels.
    pub pixmap: Pixmap,
    pub rows: Vec<RowRect>,
}

/// First row rect containing the point, if any.
pub(crate) fn hit_row(rows: &[RowRect], x: f32, y: f32) -> Option<usize> {
    rows.iter()
        .find(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
        .map(|r| r.index)
}

/// Owns the font system and glyph cache — creating a [`FontSystem`]
/// scans the system font directories, so build one per backend thread
/// and keep it for the process lifetime.
pub(crate) struct Renderer {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    /// `true` when fontconfig found at least one face. Headless test
    /// environments without fonts render empty text runs; callers that
    /// assert on text output should check this first.
    #[cfg(test)]
    pub fn has_fonts(&self) -> bool {
        self.fonts.db().faces().next().is_some()
    }

    /// Render `model` at `scale` (integer-ish device scale; logical
    /// sizes multiply by it). `hover` highlights that row.
    pub fn render(
        &mut self,
        model: &PopupModel,
        hover: Option<usize>,
        scale: f32,
    ) -> RenderedPopup {
        // A zero/negative scale would produce a zero-size pixmap (and a
        // zero font size, which cosmic-text refuses); never trust it.
        let s = if scale > 0.0 { scale } else { 1.0 };
        let n = model.entries.len();

        // Content-driven width, decided in logical px so the final
        // device size divides evenly by the scale.
        let footer = footer_text(model);
        let mut content_w = self.measure(&model.original, HEADER_FONT, Weight::NORMAL);
        content_w = content_w.max(self.measure(&footer, FOOTER_FONT, Weight::NORMAL));
        for entry in &model.entries {
            let mut w =
                BADGE_SIZE + BADGE_GAP + self.measure(&entry.text, ROW_FONT, Weight::NORMAL);
            if let Some(tag) = &entry.badge {
                w += TAG_GAP + self.measure(tag, TAG_FONT, Weight::NORMAL) + 2.0 * TAG_PAD_X;
            }
            content_w = content_w.max(w);
        }
        let panel_w = (content_w + 2.0 * PAD).clamp(MIN_W, MAX_W).ceil();
        let rows_h = n as f32 * ROW_H + n.saturating_sub(1) as f32 * ROW_GAP;
        let panel_h = (2.0 * PAD + HEADER_H + rows_h + FOOTER_H).ceil();

        let w_px = (panel_w * s).round().max(1.0) as u32;
        let h_px = (panel_h * s).round().max(1.0) as u32;
        let Some(mut pixmap) = Pixmap::new(w_px, h_px) else {
            // Unreachable with the clamps above; degrade to an empty
            // frame rather than panicking inside a paint path.
            return RenderedPopup {
                pixmap: Pixmap::new(1, 1).unwrap_or_else(|| unreachable!("1x1 pixmap")),
                rows: Vec::new(),
            };
        };

        // Panel: fill, then a 1-logical-px inner border.
        let pw = panel_w * s;
        let ph = panel_h * s;
        if let Some(path) = rounded_rect(0.5 * s, 0.5 * s, pw - s, ph - s, PANEL_RADIUS * s) {
            fill(&mut pixmap, &path, PANEL_BG);
            stroke(&mut pixmap, &path, PANEL_BORDER, s);
        }

        // Header: struck-through original word.
        let header_avail = (panel_w - 2.0 * PAD) * s;
        let header_txt = self.ellipsize(
            &model.original,
            HEADER_FONT * s,
            Weight::NORMAL,
            header_avail,
        );
        let header_w = self.measure(&header_txt, HEADER_FONT * s, Weight::NORMAL);
        self.draw_text(
            &mut pixmap,
            &header_txt,
            (
                PAD * s,
                centered_top(PAD * s, HEADER_H * s, HEADER_FONT * s),
            ),
            HEADER_FONT * s,
            HEADER_FG,
            Weight::NORMAL,
        );
        fill_rect(
            &mut pixmap,
            PAD * s,
            (PAD + HEADER_H / 2.0) * s,
            header_w,
            1.0 * s,
            HEADER_FG,
        );

        // Rows.
        let mut rows = Vec::with_capacity(n);
        let row_x = PAD * s;
        let row_w = (panel_w - 2.0 * PAD) * s;
        for (i, entry) in model.entries.iter().enumerate() {
            let row_y = (PAD + HEADER_H + i as f32 * (ROW_H + ROW_GAP)) * s;
            let hovered = hover == Some(i);

            // Hairline above the first action row — separates the
            // "replace with…" block from the "do something else"
            // block.
            if entry.is_action
                && model
                    .entries
                    .get(i.wrapping_sub(1))
                    .is_some_and(|p| !p.is_action)
            {
                if let Some(rect) =
                    tiny_skia::Rect::from_xywh(row_x, row_y - (ROW_GAP / 2.0) * s, row_w, 1.0 * s)
                {
                    let path = tiny_skia::PathBuilder::from_rect(rect);
                    fill(&mut pixmap, &path, DIVIDER);
                }
            }

            if hovered {
                if let Some(path) = rounded_rect(row_x, row_y, row_w, ROW_H * s, HOVER_RADIUS * s) {
                    fill(&mut pixmap, &path, HOVER_BG);
                }
            }

            // Digit badge.
            let badge_y = row_y + (ROW_H - BADGE_SIZE) / 2.0 * s;
            if let Some(path) = rounded_rect(
                row_x,
                badge_y,
                BADGE_SIZE * s,
                BADGE_SIZE * s,
                BADGE_RADIUS * s,
            ) {
                fill(
                    &mut pixmap,
                    &path,
                    if hovered { BADGE_BG_HOVER } else { BADGE_BG },
                );
            }
            let digit = (i + 1).to_string();
            let digit_w = self.measure(&digit, BADGE_FONT * s, Weight::BOLD);
            self.draw_text(
                &mut pixmap,
                &digit,
                (
                    row_x + (BADGE_SIZE * s - digit_w) / 2.0,
                    centered_top(badge_y, BADGE_SIZE * s, BADGE_FONT * s),
                ),
                BADGE_FONT * s,
                BADGE_FG,
                Weight::BOLD,
            );

            // Right-aligned layout-switch tag, in a subtle pill.
            let mut text_avail = row_w - (BADGE_SIZE + BADGE_GAP) * s;
            if let Some(tag) = &entry.badge {
                let tag_w = self.measure(tag, TAG_FONT * s, Weight::NORMAL);
                let pill_w = tag_w + 2.0 * TAG_PAD_X * s;
                let pill_h = TAG_FONT * s * LINE_HEIGHT_FACTOR + 2.0 * TAG_PAD_Y * s;
                let pill_x = row_x + row_w - pill_w;
                let pill_y = row_y + (ROW_H * s - pill_h) / 2.0;
                if let Some(path) = rounded_rect(pill_x, pill_y, pill_w, pill_h, TAG_RADIUS * s) {
                    fill(&mut pixmap, &path, TAG_BG);
                }
                self.draw_text(
                    &mut pixmap,
                    tag,
                    (
                        pill_x + TAG_PAD_X * s,
                        centered_top(pill_y, pill_h, TAG_FONT * s),
                    ),
                    TAG_FONT * s,
                    TAG_FG,
                    Weight::NORMAL,
                );
                text_avail -= pill_w + TAG_GAP * s;
            }

            let text = self.ellipsize(&entry.text, ROW_FONT * s, Weight::NORMAL, text_avail);
            self.draw_text(
                &mut pixmap,
                &text,
                (
                    row_x + (BADGE_SIZE + BADGE_GAP) * s,
                    centered_top(row_y, ROW_H * s, ROW_FONT * s),
                ),
                ROW_FONT * s,
                if entry.is_action { ACTION_FG } else { ROW_FG },
                Weight::NORMAL,
            );

            rows.push(RowRect {
                index: i,
                x: row_x,
                y: row_y,
                w: row_w,
                h: ROW_H * s,
            });
        }

        // Footer.
        let footer_y = (PAD + HEADER_H + rows_h) * s;
        self.draw_text(
            &mut pixmap,
            &footer,
            (
                PAD * s,
                centered_top(footer_y, FOOTER_H * s, FOOTER_FONT * s),
            ),
            FOOTER_FONT * s,
            FOOTER_FG,
            Weight::NORMAL,
        );

        RenderedPopup { pixmap, rows }
    }

    /// Width of a single shaped line, in the same units as `font_size`.
    fn measure(&mut self, text: &str, font_size: f32, weight: Weight) -> f32 {
        let buffer = self.shaped(text, font_size, weight);
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max)
    }

    /// Trim with `…` until the line fits `max_w`. Shapes per attempt,
    /// which is fine at popup sizes (≤ 9 short rows).
    fn ellipsize(&mut self, text: &str, font_size: f32, weight: Weight, max_w: f32) -> String {
        if self.measure(text, font_size, weight) <= max_w {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while chars.pop().is_some() {
            let mut candidate: String = chars.iter().collect();
            candidate.push(ELLIPSIS);
            if chars.is_empty() || self.measure(&candidate, font_size, weight) <= max_w {
                return candidate;
            }
        }
        ELLIPSIS.to_string()
    }

    fn shaped(&mut self, text: &str, font_size: f32, weight: Weight) -> Buffer {
        let mut buffer = Buffer::new(
            &mut self.fonts,
            Metrics::new(font_size, font_size * LINE_HEIGHT_FACTOR),
        );
        buffer.set_size(&mut self.fonts, None, None);
        buffer.set_text(
            &mut self.fonts,
            text,
            Attrs::new().family(Family::SansSerif).weight(weight),
            // Advanced shaping: Cyrillic, apostrophes, combining marks.
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }

    /// Draw one line with its top-left at `pos` (device px).
    fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        pos: (f32, f32),
        font_size: f32,
        color: Rgba,
        weight: Weight,
    ) {
        let buffer = self.shaped(text, font_size, weight);
        for run in buffer.layout_runs() {
            let baseline = pos.1 + run.line_y;
            for glyph in run.glyphs.iter() {
                let phys = glyph.physical((pos.0, 0.0), 1.0);
                if let Some(image) = self.cache.get_image(&mut self.fonts, phys.cache_key) {
                    // Color glyphs (emoji) are skipped: the popup shows
                    // dictionary words, and a missing emoji beats
                    // pulling per-pixel color compositing in here.
                    if image.content != SwashContent::Mask {
                        continue;
                    }
                    blit_mask(
                        pixmap,
                        (
                            phys.x + image.placement.left,
                            baseline.round() as i32 + phys.y - image.placement.top,
                        ),
                        (image.placement.width, image.placement.height),
                        &image.data,
                        color,
                    );
                }
            }
        }
    }
}

fn footer_text(model: &PopupModel) -> String {
    match &model.accept_hint {
        Some(hint) => format!("{hint}+1\u{2026}{} · click to replace", model.entries.len()),
        None => "click to replace".to_string(),
    }
}

/// Top edge that vertically centers a `font_size`-tall line (with its
/// standard line height) inside a box starting at `box_y`.
fn centered_top(box_y: f32, box_h: f32, font_size: f32) -> f32 {
    box_y + (box_h - font_size * LINE_HEIGHT_FACTOR) / 2.0
}

/// Alpha-blend an 8-bit coverage mask into the premultiplied-RGBA
/// pixmap using the standard `src + dst × (1 − src_a)` over operator.
fn blit_mask(pixmap: &mut Pixmap, origin: (i32, i32), size: (u32, u32), mask: &[u8], color: Rgba) {
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

fn fill(pixmap: &mut Pixmap, path: &Path, color: Rgba) {
    pixmap.fill_path(
        path,
        &paint(color),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn stroke(pixmap: &mut Pixmap, path: &Path, color: Rgba, width: f32) {
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint(color), &stroke, Transform::identity(), None);
}

fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
    let Some(rect) = Rect::from_xywh(x, y, w, h) else {
        return;
    };
    pixmap.fill_rect(rect, &paint(color), Transform::identity(), None);
}

/// Rounded rectangle via cubic quarter-arcs (the classic 0.5523 kappa).
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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
