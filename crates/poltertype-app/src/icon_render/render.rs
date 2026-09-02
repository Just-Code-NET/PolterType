//! Tray-icon rasteriser: layout code on a coloured square, or on
//! nothing at all.

use super::*;
use anyhow::{Context, Result};
use poltertype_core::settings::TrayIconStyle;
use poltertype_types::LayoutId;
use tray_icon::Icon;

/// Which way round a `mono` icon has to read.
///
/// Sampled once, from the desktop's own dark/light preference — a
/// panel is free to disagree, which is what the halo is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPolarity {
    Dark,
    Light,
}

impl PanelPolarity {
    pub fn from_prefers_dark(dark: bool) -> Self {
        if dark { Self::Dark } else { Self::Light }
    }

    fn letters(self) -> [u8; 4] {
        match self {
            Self::Dark => MONO_ON_DARK,
            Self::Light => MONO_ON_LIGHT,
        }
    }

    fn halo(self) -> [u8; 4] {
        match self {
            Self::Dark => MONO_HALO_ON_DARK,
            Self::Light => MONO_HALO_ON_LIGHT,
        }
    }
}

/// Build a tray icon for `layout`: its two-letter short code over a
/// colour derived from the id, grey with a pause mark when `paused`.
///
/// `mono` is the exception and draws no background at all — see
/// [`TRANSPARENT`]. Pausing still greys a `color` badge; on `mono`,
/// where there is nothing to grey, the pause bars are the whole of the
/// signal.
///
/// `flag` draws the layout's country instead of naming it, and falls
/// back to the `color` badge for a country [`flag`](super::flag) has
/// no drawing for.
///
/// A `Hidden` style still renders one. The icon is built and then
/// hidden, so turning it back on is a config change rather than a
/// restart.
pub fn for_layout(
    layout: &LayoutId,
    paused: bool,
    waiting: bool,
    style: TrayIconStyle,
    polarity: PanelPolarity,
) -> Result<Icon> {
    let mut buf = match style {
        TrayIconStyle::Flag => match flag::render(layout, paused, polarity) {
            Some(mut buf) => {
                // The flag is greyed rather than replaced, so the bars
                // take the colour they take on any greyed badge.
                if paused {
                    draw_pause_indicator(&mut buf, glyph_colour(PAUSED_BG));
                }
                buf
            }
            None => colour_badge(layout, paused),
        },
        TrayIconStyle::Color => colour_badge(layout, paused),
        TrayIconStyle::Mono | TrayIconStyle::Hidden => mono_badge(layout, paused, polarity),
    };
    if waiting {
        draw_waiting_badge(&mut buf);
    }
    Icon::from_rgba(buf, W, H).context("build tray icon")
}

fn colour_badge(layout: &LayoutId, paused: bool) -> Vec<u8> {
    let bg = if paused { PAUSED_BG } else { color_for(layout) };
    let mut buf = render(layout_short_code(layout).as_bytes(), bg);
    if paused {
        draw_pause_indicator(&mut buf, glyph_colour(bg));
    }
    buf
}

fn mono_badge(layout: &LayoutId, paused: bool, polarity: PanelPolarity) -> Vec<u8> {
    let mut buf = render_bare(layout_short_code(layout).as_bytes(), polarity);
    if paused {
        draw_pause_indicator(&mut buf, polarity.letters());
    }
    buf
}

/// Generic boot-time icon for "no layout known yet".
pub fn unknown(waiting: bool) -> Result<Icon> {
    let mut buf = render(b"??", [0x55, 0x55, 0x55, 0xFF]);
    if waiting {
        draw_waiting_badge(&mut buf);
    }
    Icon::from_rgba(buf, W, H).context("build placeholder tray icon")
}

pub(crate) fn layout_short_code(id: &LayoutId) -> String {
    let s = id.as_str();
    // Opaque IDs (`hkl:00000409`, `com.apple.keylayout.US`) — render
    // a placeholder rather than misleading letters.
    if s.contains(':') || s.contains('.') {
        return "??".into();
    }
    let primary = s.split('-').next().unwrap_or("");
    let out: String = primary
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(2)
        .collect::<String>()
        .to_ascii_uppercase();
    if out.len() < 2 { "??".into() } else { out }
}

/// Pick a background colour deterministically from the layout id —
/// the user's eye learns which colour means which language even
/// before reading the glyph.
pub(crate) fn color_for(id: &LayoutId) -> [u8; 4] {
    // FNV-1a-ish hash of the BCP-47 string, mapped into a comfortable
    // mid-saturation palette.
    let mut h: u32 = 0x811C_9DC5;
    for b in id.as_str().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    let palette: [[u8; 3]; 8] = [
        [0x4F, 0x9D, 0xFF], // blue
        [0x4F, 0xC1, 0x71], // green
        [0xE7, 0x4C, 0x3C], // red
        [0xF3, 0x9C, 0x12], // orange
        [0x9B, 0x59, 0xB6], // purple
        [0x16, 0xA0, 0x85], // teal
        [0xE2, 0x4F, 0x95], // pink
        [0x34, 0x49, 0x5E], // slate
    ];
    let [r, g, b] = palette[(h as usize) % palette.len()];
    [r, g, b, 0xFF]
}

pub(crate) fn render(text: &[u8], bg: [u8; 4]) -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    fill(&mut buf, bg);
    let fg = glyph_colour(bg);
    let (g0, g1) = two_glyphs(text);
    draw_glyph(&mut buf, g0, GLYPH_X, GLYPH_Y, fg);
    draw_glyph(&mut buf, g1, GLYPH_X + 5, GLYPH_Y, fg);
    buf
}

/// The same two letters with nothing behind them — the `mono` style.
///
/// Every glyph pixel is haloed first, in the other polarity, so the
/// letters keep an edge on a panel whose colour we guessed wrong.
pub(crate) fn render_bare(text: &[u8], polarity: PanelPolarity) -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    fill(&mut buf, TRANSPARENT);
    let (g0, g1) = two_glyphs(text);
    for (ch, x) in [(g0, GLYPH_X), (g1, GLYPH_X + 5)] {
        draw_glyph_halo(&mut buf, ch, x, GLYPH_Y, polarity.halo());
    }
    for (ch, x) in [(g0, GLYPH_X), (g1, GLYPH_X + 5)] {
        draw_glyph(&mut buf, ch, x, GLYPH_Y, polarity.letters());
    }
    buf
}

/// Centred on the design grid: content is 4 + 1 + 4 units wide, so
/// `(16 - 9) / 2` across; the glyphs are 6 tall, so `(16 - 6) / 2` down.
const GLYPH_X: i32 = 3;
const GLYPH_Y: i32 = 5;

fn two_glyphs(text: &[u8]) -> (u8, u8) {
    let chars: Vec<u8> = text.iter().take(2).copied().collect();
    (
        chars.first().copied().unwrap_or(b'?'),
        chars.get(1).copied().unwrap_or(b'?'),
    )
}

/// Which letter colour reads on `bg`.
pub(crate) fn glyph_colour(bg: [u8; 4]) -> [u8; 4] {
    if luminance(bg) > 0.55 {
        [0x10, 0x10, 0x10, 0xFF]
    } else {
        [0xFF, 0xFF, 0xFF, 0xFF]
    }
}

pub(crate) fn fill(buf: &mut [u8], rgba: [u8; 4]) {
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&rgba);
    }
}

pub(crate) fn luminance(rgba: [u8; 4]) -> f32 {
    let r = f32::from(rgba[0]) / 255.0;
    let g = f32::from(rgba[1]) / 255.0;
    let b = f32::from(rgba[2]) / 255.0;
    0.299 * r + 0.587 * g + 0.114 * b
}

/// Set one **design unit**, which is [`SCALE`]² real pixels.
///
/// Every caller works in the 16-unit grid the glyph font and the badges
/// were drawn on; this is the only place that knows the icon is bigger
/// than that.
pub(crate) fn put_pixel(buf: &mut [u8], x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= UNITS as i32 || y >= UNITS as i32 {
        return;
    }
    let (x0, y0) = (x as u32 * SCALE, y as u32 * SCALE);
    for row in y0..y0 + SCALE {
        let start = ((row * W + x0) * 4) as usize;
        for px in buf[start..start + (SCALE * 4) as usize].chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
    }
}

/// Two thin vertical bars in the bottom-right corner — the canonical
/// "paused" glyph squashed into 3x4 px so it doesn't crowd the
/// 2-letter layout code.
pub(crate) fn draw_pause_indicator(buf: &mut [u8], bar: [u8; 4]) {
    let x0 = (UNITS as i32) - 4;
    let y0 = (UNITS as i32) - 5;
    for dy in 0..4i32 {
        put_pixel(buf, x0, y0 + dy, bar);
        put_pixel(buf, x0 + 2, y0 + dy, bar);
    }
}

/// A dot in the TOP-right corner: something is waiting for the user.
///
/// Top-right because the bottom-right is the pause indicator, and the two
/// are independent — a paused PolterType with drafts waiting has to be
/// able to say both. Ringed in the background colour rather than drawn
/// flat, so it reads as a badge against any tray background and does not
/// merge into a light layout colour.
pub(crate) fn draw_waiting_badge(buf: &mut [u8]) {
    let dot = [0xFF, 0x3B, 0x30, 0xFF];
    let ring = [0xFF, 0xFF, 0xFF, 0xFF];
    let cx = (UNITS as i32) - 4;
    // Rows 0..4. The glyphs start at row 5, and the ring has to clear
    // them: at row 3 its bottom edge sat on the second glyph's top-right
    // corner, which on a two-letter code reads as part of the letter.
    let cy = 2i32;
    for dy in -2..=2i32 {
        for dx in -2..=2i32 {
            if dx.abs() == 2 && dy.abs() == 2 {
                continue;
            }
            put_pixel(buf, cx + dx, cy + dy, ring);
        }
    }
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            put_pixel(buf, cx + dx, cy + dy, dot);
        }
    }
}

/// One design unit around every pixel of the glyph, orthogonally.
///
/// Drawn before the letter itself, so the letter always sits on top of
/// its own halo rather than beside it.
pub(crate) fn draw_glyph_halo(buf: &mut [u8], ch: u8, x: i32, y: i32, halo: [u8; 4]) {
    let bits = glyph_bits(ch);
    for (row, &row_bits) in bits.iter().enumerate() {
        for col in 0..4i32 {
            if row_bits & (1 << (3 - col)) == 0 {
                continue;
            }
            let (gx, gy) = (x + col, y + row as i32);
            for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                put_pixel(buf, gx + dx, gy + dy, halo);
            }
        }
    }
}

pub(crate) fn draw_glyph(buf: &mut [u8], ch: u8, x: i32, y: i32, fg: [u8; 4]) {
    let bits = glyph_bits(ch);
    // 4 columns × 6 rows, packed row-major; bit 3 = leftmost column.
    for (row, &row_bits) in bits.iter().enumerate() {
        for col in 0..4i32 {
            if row_bits & (1 << (3 - col)) != 0 {
                put_pixel(buf, x + col, y + row as i32, fg);
            }
        }
    }
}

/// 4×6 monospace bitmap glyph for `ch`. Each row is a u8 whose low 4
/// bits hold the column pattern; bit 3 = leftmost column.
pub(crate) fn glyph_bits(ch: u8) -> [u8; 6] {
    match ch {
        b'A' => [0b0110, 0b1001, 0b1001, 0b1111, 0b1001, 0b1001],
        b'B' => [0b1110, 0b1001, 0b1110, 0b1001, 0b1001, 0b1110],
        b'C' => [0b0111, 0b1000, 0b1000, 0b1000, 0b1000, 0b0111],
        b'D' => [0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0b1110],
        b'E' => [0b1111, 0b1000, 0b1110, 0b1000, 0b1000, 0b1111],
        b'F' => [0b1111, 0b1000, 0b1110, 0b1000, 0b1000, 0b1000],
        b'G' => [0b0111, 0b1000, 0b1011, 0b1001, 0b1001, 0b0111],
        b'H' => [0b1001, 0b1001, 0b1111, 0b1001, 0b1001, 0b1001],
        b'I' => [0b1110, 0b0100, 0b0100, 0b0100, 0b0100, 0b1110],
        b'J' => [0b0001, 0b0001, 0b0001, 0b0001, 0b1001, 0b0110],
        b'K' => [0b1001, 0b1010, 0b1100, 0b1010, 0b1001, 0b1001],
        b'L' => [0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0b1111],
        b'M' => [0b1001, 0b1111, 0b1111, 0b1001, 0b1001, 0b1001],
        b'N' => [0b1001, 0b1101, 0b1111, 0b1011, 0b1001, 0b1001],
        b'O' => [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
        b'P' => [0b1110, 0b1001, 0b1001, 0b1110, 0b1000, 0b1000],
        b'Q' => [0b0110, 0b1001, 0b1001, 0b1011, 0b1010, 0b0101],
        b'R' => [0b1110, 0b1001, 0b1001, 0b1110, 0b1010, 0b1001],
        b'S' => [0b0111, 0b1000, 0b0110, 0b0001, 0b0001, 0b1110],
        b'T' => [0b1111, 0b0100, 0b0100, 0b0100, 0b0100, 0b0100],
        b'U' => [0b1001, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
        b'V' => [0b1001, 0b1001, 0b1001, 0b1001, 0b0110, 0b0110],
        b'W' => [0b1001, 0b1001, 0b1001, 0b1111, 0b1111, 0b1001],
        b'X' => [0b1001, 0b1001, 0b0110, 0b0110, 0b1001, 0b1001],
        b'Y' => [0b1001, 0b1001, 0b0110, 0b0100, 0b0100, 0b0100],
        b'Z' => [0b1111, 0b0001, 0b0010, 0b0100, 0b1000, 0b1111],
        b'0' => [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
        b'1' => [0b0010, 0b0110, 0b0010, 0b0010, 0b0010, 0b0111],
        b'2' => [0b1110, 0b0001, 0b0010, 0b0100, 0b1000, 0b1111],
        b'3' => [0b1110, 0b0001, 0b0110, 0b0001, 0b0001, 0b1110],
        b'4' => [0b1001, 0b1001, 0b1111, 0b0001, 0b0001, 0b0001],
        b'5' => [0b1111, 0b1000, 0b1110, 0b0001, 0b0001, 0b1110],
        b'6' => [0b0111, 0b1000, 0b1110, 0b1001, 0b1001, 0b0110],
        b'7' => [0b1111, 0b0001, 0b0010, 0b0100, 0b1000, 0b1000],
        b'8' => [0b0110, 0b1001, 0b0110, 0b1001, 0b1001, 0b0110],
        b'9' => [0b0110, 0b1001, 0b1001, 0b0111, 0b0001, 0b1110],
        _ => [0b0110, 0b1001, 0b0010, 0b0100, 0b0000, 0b0100],
    }
}
