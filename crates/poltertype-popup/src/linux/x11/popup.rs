//! The public handle; the X11 thread owns everything else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::Sender;
use tracing::warn;

use super::enums::{Cmd, X11PopupError};
use super::state::run;
use crate::enums::PopupUiEvent;
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

pub struct X11Popup {
    cmds: Sender<Cmd>,
    send_failed: AtomicBool,
}

impl X11Popup {
    pub fn try_new(events: Sender<PopupUiEvent>) -> Result<Self, X11PopupError> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("poltertype-popup-x11".into())
            .spawn(move || run(conn, screen_num, cmd_rx, events))
            .map_err(X11PopupError::Spawn)?;
        Ok(Self {
            cmds: cmd_tx,
            send_failed: AtomicBool::new(false),
        })
    }

    fn send(&self, cmd: Cmd) {
        if self.cmds.send(cmd).is_err() && !self.send_failed.swap(true, Ordering::Relaxed) {
            warn!("x11 popup thread is gone; suggestions will not be shown");
        }
    }
}

impl SuggestionPopup for X11Popup {
    fn show(&self, model: PopupModel) {
        self.send(Cmd::Show(model));
    }

    fn hide(&self) {
        self.send(Cmd::Hide);
    }

    fn backend_name(&self) -> &'static str {
        "linux-x11-override-redirect"
    }
}
