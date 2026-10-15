//! Settings enums: the load/save error, the tray-icon style, the
//! tooltip's spot in a window and how it answers the keyboard.

use std::io::{self};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not locate a per-user config directory for poltertype")]
    NoConfigDir,
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("toml deserialize: {0}")]
    Deserialize(#[from] toml::de::Error),
    #[error("toml serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// What the tray icon looks like — `[general].tray_icon`.
///
/// The colour carries meaning: it says which layout is in force before
/// the two letters on it are legible. This trades that away for an icon
/// that sits quietly in a panel, or for no icon at all. Asked for in
/// issue #50, from a desktop that cannot hide a single tray item
/// itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayIconStyle {
    /// A hue per layout, hashed from its id.
    #[default]
    Color,
    /// One neutral badge, whatever the layout.
    Mono,
    /// The layout's country flag, where the app has a drawing for it.
    Flag,
    /// No tray icon, and so no tray menu.
    Hidden,
}

impl TrayIconStyle {
    /// Parse the `config.toml` value. Unknown strings fall back to
    /// `Color` — the same forgiving posture the rest of the schema
    /// takes towards a hand-edited file.
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "mono" => Self::Mono,
            "flag" => Self::Flag,
            "hidden" => Self::Hidden,
            _ => Self::Color,
        }
    }

    /// The canonical `config.toml` spelling.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Mono => "mono",
            Self::Flag => "flag",
            Self::Hidden => "hidden",
        }
    }
}

/// Where in the focused window the suggestion tooltip sits when it is
/// not at the text cursor — `[suggestions].window_anchor`, one cell of
/// a 3×3 grid (issue #81).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipSpot {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    /// Just above the window's bottom edge, centred — the
    /// neighbourhood of chat inputs and shell prompts, and where the
    /// tooltip went before the choice existed.
    #[default]
    BottomCenter,
    BottomRight,
}

impl TooltipSpot {
    /// Parse the `config.toml` value. Unknown strings fall back to the
    /// default, like every other hand-editable choice in the schema.
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "top_left" => Self::TopLeft,
            "top_center" => Self::TopCenter,
            "top_right" => Self::TopRight,
            "middle_left" => Self::MiddleLeft,
            "center" => Self::Center,
            "middle_right" => Self::MiddleRight,
            "bottom_left" => Self::BottomLeft,
            "bottom_right" => Self::BottomRight,
            _ => Self::BottomCenter,
        }
    }

    /// The canonical `config.toml` spelling.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::TopCenter => "top_center",
            Self::TopRight => "top_right",
            Self::MiddleLeft => "middle_left",
            Self::Center => "center",
            Self::MiddleRight => "middle_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomCenter => "bottom_center",
            Self::BottomRight => "bottom_right",
        }
    }
}

/// Which keys accept a suggestion by number — `[suggestions].accept_keys`.
///
/// Asked for in issue #78: a desktop that binds `Meta+digit` itself
/// leaves the modifier chord no free combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AcceptKeys {
    /// `accept_modifiers` + `1`..`9`.
    #[default]
    ModifierDigits,
    /// Bare `F1`..`F9`. Only where the listener can keep them from the
    /// application — `F5` reloads a browser page — and the digit chord
    /// stands in everywhere else.
    FunctionKeys,
}

impl AcceptKeys {
    /// Parse the `config.toml` value; anything unrecognised is the
    /// default, as everywhere else in the schema.
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "function_keys" => Self::FunctionKeys,
            _ => Self::ModifierDigits,
        }
    }

    /// The canonical `config.toml` spelling.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::ModifierDigits => "modifier_digits",
            Self::FunctionKeys => "function_keys",
        }
    }
}

/// Moving through the tooltip from the keyboard —
/// `[suggestions].navigation` (issue #80).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionNavigation {
    #[default]
    Off,
    /// While the tooltip is up, `Down` / `Up` highlight a row and
    /// `Enter` accepts it. The arrows stop moving the caret for that
    /// long, which is why this is opt-in.
    Arrows,
}

impl SuggestionNavigation {
    /// Parse the `config.toml` value; anything unrecognised is `Off`.
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "arrows" => Self::Arrows,
            _ => Self::Off,
        }
    }

    /// The canonical `config.toml` spelling.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Arrows => "arrows",
        }
    }
}
