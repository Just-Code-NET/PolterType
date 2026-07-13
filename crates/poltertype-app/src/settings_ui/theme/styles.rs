//! Widget style fns for the Settings window. Each one resolves the
//! active [`BrandPalette`](super::BrandPalette) via
//! [`brand_palette`] so every widget re-themes with the window.

use iced::widget::{button, container, rule, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

use super::themes::brand_palette;

/// `c` with its alpha replaced — the "10% brand tint" trick the site
/// gets from `color-mix()`.
fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

// ── Containers ──────────────────────────────────────────────────────

/// Sidebar backdrop: raised surface. The separator against the
/// content column is a [`hairline`]-styled vertical rule, not a
/// border (iced borders are four-sided).
pub fn sidebar(theme: &Theme) -> container::Style {
    let p = brand_palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface)),
        ..container::Style::default()
    }
}

/// Card: surface colour, hairline border, the site's rounded-corner
/// radius. Groups related controls the way the landing page groups
/// feature blurbs.
pub fn card(theme: &Theme) -> container::Style {
    let p = brand_palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Keycap chip — the site's `.keycap` block: mono glyph on a raised
/// key whose "side" is a hard 2px bottom shadow.
pub fn keycap(theme: &Theme) -> container::Style {
    let p = brand_palette(theme);
    container::Style {
        text_color: Some(p.ink),
        background: Some(Background::Color(p.surface)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: Shadow {
            color: p.keycap_side,
            offset: Vector::new(0.0, 2.0),
            blur_radius: 0.0,
        },
    }
}

/// 1-px separator (sidebar edge, footer top).
pub fn hairline(theme: &Theme) -> rule::Style {
    let p = brand_palette(theme);
    rule::Style {
        color: p.line,
        width: 1,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
    }
}

// ── Buttons ─────────────────────────────────────────────────────────

/// Filled brand button — Save, Add. The site's "Download" button.
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = brand_palette(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => p.brand_hi,
        button::Status::Disabled => with_alpha(p.brand, 0.4),
        button::Status::Active => p.brand,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Quiet bordered button — Reload, Rebind, folder shortcuts.
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = brand_palette(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => p.line,
        _ => p.surface,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: p.ink,
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..button::Style::default()
    }
}

/// Destructive action with a garble-pink accent — Reset to defaults.
/// Bordered while idle, filled on hover so the destructive colour is
/// visible before commitment but loud at the moment of it.
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = brand_palette(theme);
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (p.garble, Color::WHITE),
        _ => (Color::TRANSPARENT, p.garble),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: p.garble,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..button::Style::default()
    }
}

/// Small "×" remove button on list rows: invisible until hovered,
/// then a soft garble tint — a whole row of solid danger buttons
/// would shout over the content.
pub fn danger_icon(theme: &Theme, status: button::Status) -> button::Style {
    let p = brand_palette(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => with_alpha(p.garble, 0.15),
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: p.garble,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Sidebar navigation item. Selected gets a soft brand tint (the
/// site's hover treatment on nav links); idle rows are quiet muted
/// text that ink up on hover.
pub fn nav(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = brand_palette(theme);
        let (background, text_color) = if selected {
            (with_alpha(p.brand, 0.14), p.brand)
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => {
                    (with_alpha(p.brand, 0.07), p.ink)
                }
                _ => (Color::TRANSPARENT, p.muted),
            }
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                radius: 8.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// Segmented-picker chip (action kind, wordlist layout/kind/profile,
/// theme choice). Selected = filled brand; idle = quiet bordered.
pub fn chip(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            primary(theme, status)
        } else {
            secondary(theme, status)
        }
    }
}

/// Brand-coloured inline link — the About pane's URLs.
pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let p = brand_palette(theme);
    let text_color = match status {
        button::Status::Hovered | button::Status::Pressed => p.brand_hi,
        _ => p.brand,
    };
    button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color,
        ..button::Style::default()
    }
}

// ── Text fields ─────────────────────────────────────────────────────

/// Single-line inputs: surface field, hairline border that turns
/// brand on focus — the site's `:focus-visible` outline, inverted
/// inward.
pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = brand_palette(theme);
    let border_color = match status {
        text_input::Status::Focused => p.brand,
        text_input::Status::Hovered => p.muted,
        _ => p.line,
    };
    text_input::Style {
        background: Background::Color(p.surface),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 8.0.into(),
        },
        icon: p.muted,
        placeholder: with_alpha(p.muted, 0.7),
        value: p.ink,
        selection: with_alpha(p.brand, 0.35),
    }
}

/// The wordlist editor — same treatment as [`input`].
pub fn editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = brand_palette(theme);
    let border_color = match status {
        text_editor::Status::Focused => p.brand,
        text_editor::Status::Hovered => p.muted,
        _ => p.line,
    };
    text_editor::Style {
        background: Background::Color(p.surface),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 8.0.into(),
        },
        icon: p.muted,
        placeholder: with_alpha(p.muted, 0.7),
        value: p.ink,
        selection: with_alpha(p.brand, 0.35),
    }
}
