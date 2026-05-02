//! macOS global keyboard listener (Phase 5 fills this in).
//!
//! Will use `CGEventTapCreate(kCGSessionEventTap, ..., listenOnly)`
//! attached to the CFRunLoop of a dedicated thread. Requires the user
//! to grant Accessibility permission in System Settings → Privacy.

use crossbeam_channel::Sender;

use crate::{InputError, InputListener, KeyEvent};

pub struct MacosListener;

impl MacosListener {
    pub fn new() -> Self {
        Self
    }
}

impl InputListener for MacosListener {
    fn start(&mut self, _sink: Sender<KeyEvent>) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "macOS keyboard listener not implemented yet (Phase 5)".into(),
        ))
    }

    fn stop(&mut self) {}

    fn backend_name(&self) -> &'static str {
        "macos-cg-event-tap-stub"
    }
}
