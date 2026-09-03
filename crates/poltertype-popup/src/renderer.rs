//! Owns the font system and glyph cache and turns a [`PopupModel`]
//! into a premultiplied RGBA pixmap plus clickable row hit-boxes. Pure
//! — no OS handles — so every backend reuses it and tests run
//! headless.

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};
use tiny_skia::Pixmap;

use crate::consts::{
    ACTION_FG, BADGE_BG, BADGE_BG_HOVER, BADGE_FG, BADGE_FONT, BADGE_GAP, BADGE_RADIUS, BADGE_SIZE,
    DIVIDER, ELLIPSIS, FOOTER_FG, FOOTER_FONT, FOOTER_H, HEADER_FG, HEADER_FONT, HEADER_H,
    HOVER_BG, HOVER_RADIUS, LINE_HEIGHT_FACTOR, MAX_W, MIN_W, PAD, PANEL_BG, PANEL_BORDER,
    PANEL_RADIUS, ROW_FG, ROW_FONT, ROW_GAP, ROW_H, Rgba, TAG_BG, TAG_FG, TAG_FONT, TAG_GAP,
    TAG_PAD_X, TAG_PAD_Y, TAG_RADIUS,
};
use crate::render::{blit_mask, centered_top, fill, fill_rect, footer_text, rounded_rect, stroke};
use crate::types::{PopupModel, RenderedPopup, RowRect};

/// Creating a [`FontSystem`] scans the system font directories, so
/// build one per backend thread and keep it for the process lifetime.
pub(crate) struct Renderer {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Renderer {
    pub fn new() -> Self {
        let mut fonts = FontSystem::new();
        // Every string below asks for `Family::SansSerif`, and
        // cosmic-text resolves that to the *name* "Fira Sans", which
        // most machines do not have — the request then falls through
        // to whatever the font database answers with, which on Ubuntu
        // 26.04 was a face with no text glyphs. Point the generic at
        // what this desktop actually calls its sans-serif.
        if let Some(family) = poltertype_shell::ui_font_family() {
            fonts.db_mut().set_sans_serif_family(family);
        }
        Self {
            fonts,
            cache: SwashCache::new(),
        }
    }

    /// `true` when fontconfig found at least one face. A headless
    /// environment without fonts renders empty text runs, so tests
    /// asserting on text output must check this first.
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

        let pw = panel_w * s;
        let ph = panel_h * s;
        if let Some(path) = rounded_rect(0.5 * s, 0.5 * s, pw - s, ph - s, PANEL_RADIUS * s) {
            fill(&mut pixmap, &path, PANEL_BG);
            stroke(&mut pixmap, &path, PANEL_BORDER, s);
        }

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

        let mut rows = Vec::with_capacity(n);
        let row_x = PAD * s;
        let row_w = (panel_w - 2.0 * PAD) * s;
        for (i, entry) in model.entries.iter().enumerate() {
            let row_y = (PAD + HEADER_H + i as f32 * (ROW_H + ROW_GAP)) * s;
            let hovered = hover == Some(i);

            // Hairline separating the "replace with…" rows from the
            // "do something else" ones.
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

    /// Trim with `…` until the line fits `max_w`. Re-shapes per
    /// attempt, which is fine at popup sizes (≤ 9 short rows).
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
                    // Color glyphs (emoji) skipped: the popup shows
                    // dictionary words, and a missing emoji beats
                    // pulling per-pixel colour compositing in here.
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
