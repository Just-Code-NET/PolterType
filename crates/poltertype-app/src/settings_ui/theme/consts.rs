//! Palette values — token-for-token from
//! `poltertype-web/src/styles/global.css` (`:root` and
//! `[data-theme='dark']`). If the site's tokens change, change these
//! in the same commit spirit (two repos, so: same day).

use std::sync::OnceLock;

use iced::font::Weight;
use iced::{Color, Font};

use super::types::BrandPalette;

/// A `const` mirror of `Color::from_rgb8`, which is not a `const fn` in
/// iced 0.13 — so the palettes below can be `const` items written in
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

/// The family every label in this window is drawn in, resolved once
/// against what the machine actually has.
///
/// Not a const, because iced's `Font::DEFAULT` is the *name* "Fira
/// Sans" and a machine without it gets whatever the font database
/// happens to answer — on Ubuntu 26.04 a face with no text glyphs, so
/// every label that had not named a font came out blank. See
/// `poltertype_shell::ui_font_family`. Falls back to iced's own
/// default where there is nothing better to ask.
pub fn font_ui() -> Font {
    static FAMILY: OnceLock<Option<&'static str>> = OnceLock::new();
    // Leaked on purpose: iced wants a `&'static str`, and this is one
    // short string for the lifetime of a process that owns one window.
    let family = FAMILY.get_or_init(|| {
        poltertype_shell::ui_font_family().map(|f| &*Box::leak(f.into_boxed_str()))
    });
    family.map_or(Font::DEFAULT, Font::with_name)
}

/// Bold cut of the UI font — headers and the wordmark. (The site uses
/// Bricolage Grotesque here; bundling a display font into the binary
/// isn't worth ~300 KB for a rarely-opened window.)
pub fn font_bold() -> Font {
    Font {
        weight: Weight::Bold,
        ..font_ui()
    }
}

/// Fixed-width, for text a program produced rather than text we wrote:
/// a plug-in's report, where columns only line up if the glyphs do.
/// `MONOSPACE` is a family request, so it resolves to whatever the
/// system has and bundles nothing.
pub const FONT_MONO: Font = Font::MONOSPACE;
