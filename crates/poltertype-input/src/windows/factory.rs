//! Constructors for the Windows listener, emitter and key gate.

use std::sync::Arc;

use crate::{InputError, InputListener, KeyEmitter, KeyGate};

use super::{WindowsEmitter, WindowsGate, WindowsListener};

pub(crate) fn create_key_gate(_hold_keys: bool) -> KeyGate {
    KeyGate::with_backend(Arc::new(WindowsGate::new()))
}

pub(crate) fn create_listener(gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    // The hook callback needs the gate to decide what to swallow;
    // without it the listener observes and never blocks.
    Ok(Box::new(match gate.backend() {
        Some(g) => WindowsListener::with_gate(Arc::clone(g)),
        None => WindowsListener::new(),
    }))
}

pub(crate) fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    Ok(Box::new(WindowsEmitter::new()))
}
