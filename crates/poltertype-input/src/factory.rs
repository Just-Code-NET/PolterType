//! Per-OS constructors for the listener and emitter.

use crate::*;

/// Construct the listener appropriate for the current OS.
pub fn create_listener() -> Result<Box<dyn InputListener>, InputError> {
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
        linux::create_listener()
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
