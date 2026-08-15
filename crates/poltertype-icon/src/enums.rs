//! What can go wrong rendering the icon.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("icon size must be at least {min} px (got {got})")]
    TooSmall { min: u32, got: u32 },

    #[error("write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("encode PNG: {0}")]
    Png(#[from] png::EncodingError),
}
