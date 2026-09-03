//! X11 backend errors and thread commands.

use thiserror::Error;

use crate::types::PopupModel;

#[derive(Debug, Error)]
pub enum X11PopupError {
    #[error("x11 connect: {0}")]
    Connect(#[from] x11rb::errors::ConnectError),
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}

pub(super) enum Cmd {
    Show(PopupModel),
    Hide,
}
