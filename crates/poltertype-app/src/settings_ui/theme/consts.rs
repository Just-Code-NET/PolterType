//! Palette values — token-for-token from
//! `poltertype-web/src/styles/global.css` (`:root` and
//! `[data-theme='dark']`). If the site's tokens change, change these
//! in the same commit spirit (two repos, so: same day).

use iced::font::Weight;
use iced::{Color, Font};

use super::types::BrandPalette;

/// `Color` from 8-bit sRGB components. A `const` mirror of
/// `Color::from_rgb8`, which isn't a `const fn` in iced 0.13 — this
/// exists only so the palettes below can be `const` items written in
/// the same hex notation as the CSS they mirror.
const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

/// The site's `:root` (light) tokens.
pub const LIGHT: BrandPalette = BrandPalette {
    bg: rgb8(0xF7, 0xF6, 0xFD),
    surface: rgb8(0xFF, 0xFF, 0xFF),
    ink: rgb8(0x17, 0x14, 0x3A),
    muted: rgb8(0x5B, 0x56, 0x87),
    brand: rgb8(0x4F, 0x46, 0xE5),
    brand_hi: rgb8(0x6D, 0x65, 0xF2),
    ecto: rgb8(0x0A, 0x8F, 0x63),
    garble: rgb8(0xD3, 0x17, 0x5C),
    line: rgb8(0xE5, 0xE2, 0xF5),
    warn: rgb8(0xA1, 0x62, 0x07),
    keycap_side: rgb8(0xDC, 0xD9, 0xF0),
};

/// The site's `[data-theme='dark']` tokens.
pub const DARK: BrandPalette = BrandPalette {
    bg: rgb8(0x0B, 0x0A, 0x15),
    surface: rgb8(0x14, 0x12, 0x2A),
    ink: rgb8(0xEC, 0xEA, 0xFB),
    muted: rgb8(0x9D, 0x98, 0xC6),
    brand: rgb8(0x6B, 0x62, 0xF6),
    brand_hi: rgb8(0x8A, 0x82, 0xFF),
    ecto: rgb8(0x3E, 0xE6, 0xA0),
    garble: rgb8(0xFF, 0x7A, 0x95),
    line: rgb8(0x27, 0x23, 0x48),
    warn: rgb8(0xFB, 0xBF, 0x24),
    keycap_side: rgb8(0x0E, 0x0C, 0x1F),
};

// ── GhostMark colours (GhostMark.astro) ─────────────────────────────
// Fixed across themes, exactly like the site: the mark is always an
// indigo keycap with a pale ghost, whatever the page theme.

/// Top face of the keycap (`#6D65F2`).
pub const MARK_KEYCAP_TOP: Color = rgb8(0x6D, 0x65, 0xF2);
/// Keycap side + inner face (`#4F46E5`).
pub const MARK_KEYCAP_FACE: Color = rgb8(0x4F, 0x46, 0xE5);
/// The ghost's body (`#F7F6FD`).
pub const MARK_GHOST: Color = rgb8(0xF7, 0xF6, 0xFD);
/// Eyes and smile (`#17143A`).
pub const MARK_FACE: Color = rgb8(0x17, 0x14, 0x3A);

/// Bold cut of the default UI font — headers and the wordmark. (The
/// site uses Bricolage Grotesque here; bundling a display font into
/// the binary isn't worth ~300 KB for a rarely-opened window.)
pub const FONT_BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// Fixed-width, for text a program produced rather than text we wrote:
/// a plug-in's report, where columns only line up if the glyphs do.
/// `MONOSPACE` is a family request, so it resolves to whatever the
/// system has and bundles nothing.
pub const FONT_MONO: Font = Font::MONOSPACE;
