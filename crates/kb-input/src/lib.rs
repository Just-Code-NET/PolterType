//! Per-OS global keyboard listener.
//!
//! Public surface:
//! * [`InputListener`] — trait every per-OS implementation satisfies.
//! * [`create_listener`] — runtime factory that picks the right backend.
//! * [`KeyEvent`] — re-exported from `kb-types`.
//!
//! ## Threading model
//!
//! `InputListener::start` may spawn its own dedicated thread (Windows
//! does, because `WH_KEYBOARD_LL` requires an OS message loop on the
//! installing thread). The listener pushes events into a
//! [`crossbeam_channel::Sender`] supplied by the caller — never blocking
//! and never doing any non-trivial work in the OS hook callback.

#![deny(unsafe_op_in_unsafe_fn)]

use crossbeam_channel::Sender;
use thiserror::Error;

pub use kb_types::{KeyDirection, KeyEvent, Modifiers};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("the active platform does not support a global keyboard listener: {0}")]
    Unsupported(String),
    #[error("OS error while installing keyboard hook: {0}")]
    Os(String),
    #[error("listener already started")]
    AlreadyStarted,
}

/// A per-OS global keyboard listener.
///
/// Implementations must be `Send` so they can be moved onto a worker
/// thread. They are not required to be `Sync`; only one task drives the
/// listener at a time.
pub trait InputListener: Send {
    /// Start delivering events into `sink`. Returns once the OS hook
    /// is installed (or fails). The listener owns the worker thread
    /// for its lifetime.
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError>;

    /// Stop and tear down the OS hook. Idempotent.
    fn stop(&mut self);

    /// Human-readable backend name (e.g. `"windows-ll-hook"`,
    /// `"linux-evdev"`). Useful for logs and the tray onboarding banner.
    fn backend_name(&self) -> &'static str;
}

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

// ─── KeyEmitter ──────────────────────────────────────────────────────

/// Synthesises keystrokes — used by the corrector to delete the
/// just-typed word and re-type it after switching layouts.
///
/// All emitted events come back through [`InputListener`] with
/// `injected = true`; the engine drops those to avoid feedback.
pub trait KeyEmitter: Send + Sync {
    /// Emit `n` Backspace presses, one after another.
    fn send_backspaces(&self, n: usize) -> Result<(), InputError>;

    /// Emit `text` as Unicode characters. On Windows uses
    /// `KEYEVENTF_UNICODE`, which is layout-independent.
    fn send_text(&self, text: &str) -> Result<(), InputError>;

    fn backend_name(&self) -> &'static str;
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
