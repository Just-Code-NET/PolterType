//! Placeholder app-icon rasteriser.

use super::*;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

/// Render `size`×`size` RGBA PNG to `out`.
pub fn render_app_icon(size: u32, out: &Path) -> Result<()> {
    if size < 32 {
        anyhow::bail!("icon size must be ≥ 32 px (got {size})");
    }
    let n = size as usize;
    let mut buf = vec![0u8; n * n * 4]; // start fully transparent

    fill_rounded_square(&mut buf, n, &INDIGO, size as f32 * 0.18);
    draw_kb_wordmark(&mut buf, n, &WHITE);

    write_png(out, &buf, size, size)
}

/// Fill an anti-aliased rounded square covering the full canvas.
///
/// Uses a signed-distance-field test against the rounded square: a
/// pixel's distance to the nearest edge is positive outside and
/// negative inside; we map a 1.5-px band around the edge to an alpha
/// gradient so corners look smooth at any output size.
pub(crate) fn fill_rounded_square(buf: &mut [u8], n: usize, color: &[u8; 4], radius: f32) {
    let half = n as f32 / 2.0;
    let band = 1.5; // anti-alias half-band (in pixels)

    for y in 0..n {
        for x in 0..n {
            let xc = (x as f32 + 0.5) - half;
            let yc = (y as f32 + 0.5) - half;
            // SDF for a rounded box centred at origin, half-extent =
            // (half, half), corner radius = `radius`.
            let dx = xc.abs() - (half - radius);
            let dy = yc.abs() - (half - radius);
            let outside = dx.max(0.0).hypot(dy.max(0.0));
            let inside = dx.max(dy).min(0.0); // negative when inside
            let dist = outside + inside - radius;

            let alpha = if dist <= -band {
                1.0
            } else if dist >= band {
                0.0
            } else {
                // Linear ramp in the band — visually indistinguishable
                // from a smoothstep at this scale.
                0.5 - dist / (2.0 * band)
            };
            if alpha <= 0.0 {
                continue;
            }
            let idx = (y * n + x) * 4;
            buf[idx] = color[0];
            buf[idx + 1] = color[1];
            buf[idx + 2] = color[2];
            buf[idx + 3] = ((color[3] as f32) * alpha).clamp(0.0, 255.0) as u8;
        }
    }
}

/// Draw "kb" centred horizontally at ~62% of canvas width.
///
/// Two 5-wide glyphs + 1-col gap = 11 source columns mapped to
/// `target_w` output columns. We round `cell_w` / `cell_h` to whole
/// pixels (scaled-up nearest-neighbour) — at 1024px output that gives
/// thick, crisp strokes; at smaller sizes everything still lines up.
pub(crate) fn draw_kb_wordmark(buf: &mut [u8], n: usize, color: &[u8; 4]) {
    const COLS: usize = 11; // 5 + 1 gap + 5
    const ROWS: usize = 7;
    let target_w = (n as f32 * 0.62) as usize;
    let cell = (target_w / COLS).max(1);
    let total_w = cell * COLS;
    let total_h = cell * ROWS;
    let off_x = n.saturating_sub(total_w) / 2;
    let off_y = n.saturating_sub(total_h) / 2;

    for (rows, gx0) in [(GLYPH_K, 0usize), (GLYPH_B, 6)] {
        for (gy, &row) in rows.iter().enumerate() {
            for gx in 0..5 {
                let bit = (row >> (7 - gx)) & 1;
                if bit == 0 {
                    continue;
                }
                let px0 = off_x + (gx0 + gx) * cell;
                let py0 = off_y + gy * cell;
                for dy in 0..cell {
                    for dx in 0..cell {
                        let px = px0 + dx;
                        let py = py0 + dy;
                        if px >= n || py >= n {
                            continue;
                        }
                        let idx = (py * n + px) * 4;
                        buf[idx] = color[0];
                        buf[idx + 1] = color[1];
                        buf[idx + 2] = color[2];
                        buf[idx + 3] = color[3];
                    }
                }
            }
        }
    }
}

pub(crate) fn write_png(out: &Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    let f = File::create(out).with_context(|| format!("create {}", out.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().context("png write_header")?;
    writer
        .write_image_data(rgba)
        .context("png write_image_data")?;
    Ok(())
}
