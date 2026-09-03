//! The public handle; the popup thread owns everything else.
//!
//! One dedicated thread owns the window and the renderer; this handle
//! only pushes commands into a channel, the same shape as the two
//! Linux backends.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::Sender;
use tracing::warn;

use super::enums::{Cmd, WindowsPopupError};
use super::run::run;
use crate::enums::PopupUiEvent;
use crate::noop::NoopPopup;
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// Windows needs nothing probed: a layered topmost window exists on
/// every version we ship to. Creation can still fail — a session with
/// no interactive window station, for one — and then the tooltip
/// degrades to the keyboard accept chord as it does elsewhere.
pub(crate) fn create_for_platform(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    match WindowsPopup::try_new(events) {
        Ok(p) => Box::new(p),
        Err(e) => {
            warn!(err = %e, "layered popup unavailable");
            Box::new(NoopPopup)
        }
    }
}

/// Channel-sending handle; the popup thread owns everything else.
pub struct WindowsPopup {
    cmds: Sender<Cmd>,
    send_failed: AtomicBool,
}

impl WindowsPopup {
    pub fn try_new(events: Sender<PopupUiEvent>) -> Result<Self, WindowsPopupError> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("poltertype-popup-windows".into())
            .spawn(move || run(cmd_rx, events))
            .map_err(WindowsPopupError::Spawn)?;
        Ok(Self {
            cmds: cmd_tx,
            send_failed: AtomicBool::new(false),
        })
    }

    fn send(&self, cmd: Cmd) {
        if self.cmds.send(cmd).is_err() && !self.send_failed.swap(true, Ordering::Relaxed) {
            warn!("popup thread is gone; suggestions will not be shown");
        }
    }
}

impl SuggestionPopup for WindowsPopup {
    fn show(&self, model: PopupModel) {
        self.send(Cmd::Show(Box::new(model)));
    }

    fn hide(&self) {
        self.send(Cmd::Hide);
    }

    fn backend_name(&self) -> &'static str {
        "windows-layered-topmost"
    }
}
