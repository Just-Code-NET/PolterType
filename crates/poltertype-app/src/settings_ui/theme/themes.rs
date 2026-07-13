//! Building the branded [`Theme`]s and resolving a `&Theme` back to
//! its [`BrandPalette`].

use std::sync::LazyLock;

use iced::Theme;
use iced::theme::Palette;

use super::consts::{DARK, LIGHT};
use super::types::BrandPalette;

// `Theme::custom` derives the extended palette (hover shades, weak /
// strong background pairs, …) with a bit of colour math — cache both
// themes once instead of re-deriving on every `SettingsApp::theme()`
// call (iced asks per frame).
static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| {
    Theme::custom(
        "PolterType Light".to_owned(),
        Palette {
            background: LIGHT.bg,
            text: LIGHT.ink,
            primary: LIGHT.brand,
            success: LIGHT.ecto,
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
/// back to the full token set. Keyed off `is_dark`, which
/// `Theme::custom` derives from the background luminance, so it stays
/// correct for both our themes (and picks something sane if a stock
/// iced theme ever leaks in).
pub fn brand_palette(theme: &Theme) -> &'static BrandPalette {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}
