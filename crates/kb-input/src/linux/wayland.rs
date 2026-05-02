//! Wayland keyboard listener via `evdev` (`/dev/input/event*`).
//!
//! Wayland intentionally provides no global keyboard-snooping API —
//! this is by design of the protocol. The realistic path for a
//! desktop tool is to read the raw `evdev` devices, which requires
//! the user to be in the `input` group + a udev rule. Phase 6 ships
//! a `setup-linux.sh` that does this with a single `sudo` prompt.

use crossbeam_channel::Sender;

use crate::{InputError, InputListener, KeyEvent};

pub struct EvdevListener;

impl EvdevListener {
    pub fn new() -> Self {
        Self
    }
}

impl InputListener for EvdevListener {
    fn start(&mut self, _sink: Sender<KeyEvent>) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "Wayland evdev listener not implemented yet (Phase 6); \
             run setup-linux.sh first when it ships"
                .into(),
        ))
    }

    fn stop(&mut self) {}

    fn backend_name(&self) -> &'static str {
        "linux-wayland-evdev-stub"
    }
}
