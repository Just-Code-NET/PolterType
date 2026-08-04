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
    #[cfg(windows)]
    {
        KeyGate::windows(std::sync::Arc::new(windows::WindowsGate::new()))
    }
    #[cfg(target_os = "macos")]
    {
        KeyGate::macos(std::sync::Arc::new(macos::MacosGate::new()))
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
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
        // The hook callback needs the gate to decide what to swallow;
        // without it the listener observes and never blocks, which is
        // the behaviour before this existed.
        Ok(Box::new(match gate.windows_inner() {
            Some(g) => windows::WindowsListener::with_gate(std::sync::Arc::clone(g)),
            None => windows::WindowsListener::new(),
        }))
    }
    #[cfg(target_os = "macos")]
    {
        // Same wiring as Windows: the tap callback consults the gate
        // on every keystroke.
        Ok(Box::new(match gate.macos_inner() {
            Some(g) => macos::MacosListener::with_gate(std::sync::Arc::clone(g)),
            None => macos::MacosListener::new(),
        }))
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
