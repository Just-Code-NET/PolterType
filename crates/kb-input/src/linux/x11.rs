//! X11 keyboard listener via XInput2 + emitter via XTest.
//!
//! Stub implementation that reports `Unsupported` for now — the
//! Wayland-first plan in DECISIONS.md prioritises the evdev path,
//! and most modern distributions default to Wayland anyway. v0.1.x
//! will fill these in for users on legacy X11 sessions.

#![allow(unused_imports, dead_code)] // Linux-only.

use crossbeam_channel::Sender;

use crate::{InputError, InputListener, KeyEmitter, KeyEvent};

pub struct X11Listener;

impl X11Listener {
    pub fn new() -> Self {
        Self
    }
}

impl InputListener for X11Listener {
    fn start(&mut self, _sink: Sender<KeyEvent>) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "X11 XInput2 listener is a v0.1.x item — \
             switch to Wayland or run via XWayland-on-evdev"
                .into(),
        ))
    }

    fn stop(&mut self) {}

    fn backend_name(&self) -> &'static str {
        "linux-x11-xinput2"
    }
}

pub struct X11Emitter;

impl X11Emitter {
    pub fn new() -> Self {
        Self
    }
}

impl KeyEmitter for X11Emitter {
    fn send_backspaces(&self, _n: usize) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "X11 XTest emitter is a v0.1.x item".into(),
        ))
    }
    fn send_text(&self, _text: &str) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "X11 XTest emitter is a v0.1.x item".into(),
        ))
    }
    fn backend_name(&self) -> &'static str {
        "linux-x11-xtest"
    }
}
