//! Constructors for the macOS listener, emitter and key gate.

use std::sync::Arc;

use crate::{InputError, InputListener, KeyEmitter, KeyGate, ReplaySpeed};

use super::{MacosEmitter, MacosGate, MacosListener};

pub(crate) fn create_key_gate(hold_keys: bool) -> KeyGate {
    KeyGate::with_backend(Arc::new(MacosGate::new(hold_keys)))
}

pub(crate) fn create_listener(gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    // Same wiring as Windows: the tap callback consults the gate
    // on every keystroke.
    let listener = match gate.backend() {
        Some(g) => MacosListener::with_gate(Arc::clone(g)),
        None => MacosListener::new(),
    };
    Ok(Box::new(listener.with_claims(Arc::clone(gate.claims()))))
}

/// Only an active tap can drop an event, and the tap is active exactly
/// when the key gate is on — the same switch [`MacosGate::new`] reads.
pub(crate) fn suggestion_keys_claimable(hold_keys: bool) -> bool {
    MacosGate::enabled_by(hold_keys)
}

pub(crate) fn create_emitter(_speed: ReplaySpeed) -> Result<Box<dyn KeyEmitter>, InputError> {
    Ok(Box::new(MacosEmitter::new()))
}
