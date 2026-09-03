//! Small functions behind the palette and font constants.

use std::sync::OnceLock;

use iced::font::Weight;
use iced::{Color, Font};

/// A `const` mirror of `Color::from_rgb8`, which is not a `const fn` in
/// iced 0.13 — so the palettes in [`super::consts`] can be `const` items
/// written in the same hex notation as the CSS they mirror.
pub(super) const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

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
