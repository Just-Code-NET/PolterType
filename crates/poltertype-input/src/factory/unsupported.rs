//! Constructors used when compiled for a target with no input backend.

use crate::{InputError, InputListener, KeyEmitter, KeyGate};

pub(crate) fn create_key_gate(_hold_keys: bool) -> KeyGate {
    KeyGate::disabled()
}

pub(crate) fn create_listener(_gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    Err(InputError::Unsupported(format!(
        "unsupported target_os = {}",
        std::env::consts::OS
    )))
}

pub(crate) fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    Err(InputError::Unsupported(format!(
        "unsupported target_os = {}",
        std::env::consts::OS
    )))
}
