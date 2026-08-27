//! Building the branded [`Theme`]s and resolving a `&Theme` back to
//! its [`BrandPalette`].

use std::sync::LazyLock;

use iced::Theme;
use iced::theme::Palette;

use super::consts::{DARK, LIGHT};
use super::types::BrandPalette;

// `Theme::custom` derives the extended palette with colour math, and
// iced asks for the theme per frame — derive both once.
static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| {
    Theme::custom(
        "PolterType Light".to_owned(),
        Palette {
            background: LIGHT.bg,
            text: LIGHT.ink,
            primary: LIGHT.brand,
            success: LIGHT.ecto,
            warning: LIGHT.warn,
            danger: LIGHT.garble,
        },
    )
});

static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| {
    Theme::custom(
        "PolterType Dark".to_owned(),
        Palette {
            background: DARK.bg,
            text: DARK.ink,
            primary: DARK.brand,
            success: DARK.ecto,
            warning: DARK.warn,
            danger: DARK.garble,
        },
    )
});

/// The branded light theme (site `:root` tokens).
pub fn light() -> Theme {
    LIGHT_THEME.clone()
}

/// The branded dark theme (site `[data-theme='dark']` tokens).
pub fn dark() -> Theme {
    DARK_THEME.clone()
}

/// The brand palette matching `theme`. Style fns receive only a
/// `&Theme`, and iced's palette carries just five colours — this maps
/// back to the full token set. Keyed off `is_dark`, derived from
/// background luminance, so a stock iced theme leaking in still picks
/// something sane.
pub fn brand_palette(theme: &Theme) -> &'static BrandPalette {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}
