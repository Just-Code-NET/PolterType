//! Windows backend errors and thread commands.

use thiserror::Error;

use crate::types::PopupModel;

#[derive(Debug, Error)]
pub enum WindowsPopupError {
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}

pub(super) enum Cmd {
    Show(Box<PopupModel>),
    Hide,
}
