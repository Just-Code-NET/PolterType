//! Settings load/save error type.

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
