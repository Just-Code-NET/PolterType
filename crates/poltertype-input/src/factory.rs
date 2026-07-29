//! Per-OS constructors for the listener and emitter.

use crate::*;

/// A gate that can hold the user's keystrokes back while a correction
/// is being typed, paired with the listener [`create_listener`]
/// returns: on Linux/evdev the two share the device thread that owns
/// the grabs, so **create the gate first and pass it in**. Every other
/// backend returns a no-op gate.
///
/// Whether it can actually hold anything is only known once the
/// listener has started — see [`KeyGate::available`].
pub fn create_key_gate() -> KeyGate {
    #[cfg(target_os = "linux")]
    {
        linux::create_key_gate()
    }
    #[cfg(not(target_os = "linux"))]
    {
        KeyGate::disabled()
    }
}

/// Construct the listener appropriate for the current OS, wired to
/// `gate` where the backend supports one.
pub fn create_listener(gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    let _ = gate;
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WindowsListener::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacosListener::new()))
    }
    #[cfg(target_os = "linux")]
    {
        linux::create_listener(gate)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(InputError::Unsupported(format!(
            "unsupported target_os = {}",
            std::env::consts::OS
        )))
    }
}

pub fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WindowsEmitter::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacosEmitter::new()))
    }
    #[cfg(target_os = "linux")]
    {
        linux::create_emitter()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(InputError::Unsupported(format!(
            "unsupported target_os = {}",
            std::env::consts::OS
        )))
    }
}
