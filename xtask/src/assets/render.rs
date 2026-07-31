//! App-icon rasteriser.

use super::*;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

/// Samples per axis inside one pixel. 4×4 is plenty once a pixel is
/// under a design unit; below `FINE_DETAIL_SIZE` the smile's stroke is
/// thinner than a pixel, so those sizes get a finer grid.
const SAMPLES: u32 = 4;
const FINE_SAMPLES: u32 = 8;
const FINE_DETAIL_SIZE: u32 = 64;

/// Output sizes above this skip the sample grid on pixels whose corners
/// and centre all land on the same layer. Below it, every pixel takes
/// the full grid: the probe can step over a sub-pixel feature, and at
/// these sizes the whole image costs less than a megasample anyway.
const ADAPTIVE_FROM: u32 = 256;

/// Render `size`×`size` RGBA PNG of the app icon to `out`.
pub fn render_app_icon(size: u32, out: &Path) -> Result<()> {
    if size < 32 {
        anyhow::bail!("icon size must be ≥ 32 px (got {size})");
    }
    let buf = rasterise(size);
    write_png(out, &buf, size, size)
}

/// Rasterise the icon into a `size`×`size` RGBA buffer.
pub(crate) fn rasterise(size: u32) -> Vec<u8> {
    let n = size as usize;
    let scale = UNITS / size as f32;
    let samples = if size <= FINE_DETAIL_SIZE {
        FINE_SAMPLES
    } else {
        SAMPLES
    };
    let adaptive = size > ADAPTIVE_FROM;

    let mut buf = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let px = pixel(x as f32, y as f32, scale, samples, adaptive);
            let i = (y * n + x) * 4;
            buf[i..i + 4].copy_from_slice(&px);
        }
    }
    buf
}

/// Resolve one output pixel, anti-aliased by area sampling.
fn pixel(x: f32, y: f32, scale: f32, samples: u32, adaptive: bool) -> [u8; 4] {
    if adaptive {
        let centre = layer_at((x + 0.5) * scale, (y + 0.5) * scale);
        let uniform = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
            .iter()
            .all(|&(dx, dy)| layer_at((x + dx) * scale, (y + dy) * scale) == centre);
        if uniform {
            return centre;
        }
    }

    // Average the covered samples' colour and let coverage drive alpha.
    // Uncovered samples contribute nothing at all — folding their
    // transparent black into the average is what gives naive AA its
    // dark fringe.
    let step = 1.0 / samples as f32;
    let mut acc = [0u32; 3];
    let mut hits = 0u32;
    for sy in 0..samples {
        for sx in 0..samples {
            let u = (x + (sx as f32 + 0.5) * step) * scale;
            let v = (y + (sy as f32 + 0.5) * step) * scale;
            let c = layer_at(u, v);
            if c[3] == 0 {
                continue;
            }
            acc[0] += u32::from(c[0]);
            acc[1] += u32::from(c[1]);
            acc[2] += u32::from(c[2]);
            hits += 1;
        }
    }
    if hits == 0 {
        return TRANSPARENT;
    }
    let total = samples * samples;
    [
        (acc[0] / hits) as u8,
        (acc[1] / hits) as u8,
        (acc[2] / hits) as u8,
        (255 * hits / total) as u8,
    ]
}

/// Colour of the topmost layer covering a point, in design units.
fn layer_at(u: f32, v: f32) -> [u8; 4] {
    if in_eye(u, v) || in_smile(u, v) {
        INK
    } else if in_ghost(u, v) {
        GHOST
    } else if in_round_rect(u, v, &KEY_WELL_RECT) {
        KEY_SIDE
    } else if in_round_rect(u, v, &KEY_FACE_RECT) {
        KEY_FACE
    } else if in_round_rect(u, v, &KEY_SIDE_RECT) {
        KEY_SIDE
    } else {
        TRANSPARENT
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
