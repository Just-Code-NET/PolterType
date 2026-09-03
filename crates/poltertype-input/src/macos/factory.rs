//! Constructors for the macOS listener, emitter and key gate.

use std::sync::Arc;

use crate::{InputError, InputListener, KeyEmitter, KeyGate};

use super::{MacosEmitter, MacosGate, MacosListener};

pub(crate) fn create_key_gate(hold_keys: bool) -> KeyGate {
    KeyGate::with_backend(Arc::new(MacosGate::new(hold_keys)))
}

pub(crate) fn create_listener(gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    // Same wiring as Windows: the tap callback consults the gate
    // on every keystroke.
    Ok(Box::new(match gate.backend() {
        Some(g) => MacosListener::with_gate(Arc::clone(g)),
        None => MacosListener::new(),
    }))
}

pub(crate) fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    Ok(Box::new(MacosEmitter::new()))
}
