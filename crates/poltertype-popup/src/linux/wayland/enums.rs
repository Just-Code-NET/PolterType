//! Wayland backend errors and thread commands.

use thiserror::Error;
use wayland_client::ConnectError;
use wayland_client::globals::{BindError, GlobalError};

use crate::types::PopupModel;

#[derive(Debug, Error)]
pub enum WaylandPopupError {
    #[error("wayland connect: {0}")]
    Connect(#[from] ConnectError),
    #[error("wayland globals: {0}")]
    Globals(#[from] GlobalError),
    #[error("compositor exposes no zwlr_layer_shell_v1 (GNOME/Mutter)")]
    NoLayerShell,
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}

pub(super) enum Cmd {
    Show(PopupModel),
    Hide,
}

/// Failures while binding globals on the popup thread (after the
/// factory already accepted this backend) — logged, never surfaced.
#[derive(Debug, Error)]
pub(super) enum WlInitError {
    #[error("bind global: {0}")]
    Bind(#[from] BindError),
    #[error("create shm pool: {0}")]
    Pool(#[from] smithay_client_toolkit::shm::CreatePoolError),
}
