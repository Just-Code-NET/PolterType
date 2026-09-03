//! Windows backend errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WindowsPopupError {
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}
