//! Error types for layout loading.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LayoutLoadError {
    #[error("invalid layout TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("data directory not resolved: {0}")]
    DataDir(#[from] crate::data_dir::DataDirError),
}
