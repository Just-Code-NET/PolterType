//! Settings enums: the load/save error, and the tray-icon style.

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
            "hidden" => Self::Hidden,
            _ => Self::Color,
        }
    }

    /// The canonical `config.toml` spelling.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Mono => "mono",
            Self::Hidden => "hidden",
        }
    }
}
