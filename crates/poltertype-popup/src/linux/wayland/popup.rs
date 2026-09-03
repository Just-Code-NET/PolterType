//! The public handle; the Wayland thread owns everything else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::Sender;
use tracing::warn;
use wayland_client::Connection;
use wayland_client::globals::registry_queue_init;

use super::enums::{Cmd, WaylandPopupError};
use super::run::run;
use super::state::WlState;
use crate::enums::PopupUiEvent;
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// Channel-sending handle; the Wayland thread owns everything else.
pub struct WaylandPopup {
    cmds: Sender<Cmd>,
    send_failed: AtomicBool,
}

impl WaylandPopup {
    pub fn try_new(events: Sender<PopupUiEvent>) -> Result<Self, WaylandPopupError> {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init::<WlState>(&conn)?;
        // The factory needs "no layer-shell" distinguished from "no
        // Wayland at all" *before* we commit to this backend.
        let has_layer_shell = globals
            .contents()
            .with_list(|list| list.iter().any(|g| g.interface == "zwlr_layer_shell_v1"));
        if !has_layer_shell {
            return Err(WaylandPopupError::NoLayerShell);
        }

        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("poltertype-popup-wl".into())
            .spawn(move || run(globals, event_queue, cmd_rx, events))
            .map_err(WaylandPopupError::Spawn)?;
        Ok(Self {
            cmds: cmd_tx,
            send_failed: AtomicBool::new(false),
        })
    }

    fn send(&self, cmd: Cmd) {
        // The thread only dies on a compositor error; losing a popup
        // then is fine, but say so once.
        if self.cmds.send(cmd).is_err() && !self.send_failed.swap(true, Ordering::Relaxed) {
            warn!("wayland popup thread is gone; suggestions will not be shown");
        }
    }
}

impl SuggestionPopup for WaylandPopup {
    fn show(&self, model: PopupModel) {
        self.send(Cmd::Show(model));
    }

    fn hide(&self) {
        self.send(Cmd::Hide);
    }

    fn backend_name(&self) -> &'static str {
        "linux-wayland-layer-shell"
    }
}
