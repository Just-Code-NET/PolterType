//! X11 keyboard listener via XInput2 RawKeyPress (Phase 6 fills in).
//!
//! Fallback path for users on legacy X11 sessions; Wayland is the
//! main Linux focus.

use crossbeam_channel::Sender;

use crate::{InputError, InputListener, KeyEvent};

pub struct X11Listener;

impl X11Listener {
    pub fn new() -> Self {
        Self
    }
}

impl InputListener for X11Listener {
    fn start(&mut self, _sink: Sender<KeyEvent>) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "X11 XInput2 listener not implemented yet (Phase 6)".into(),
        ))
    }

    fn stop(&mut self) {}

    fn backend_name(&self) -> &'static str {
        "linux-x11-xinput2-stub"
    }
}
