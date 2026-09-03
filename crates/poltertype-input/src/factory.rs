//! Per-OS constructors for the listener and emitter.

#[cfg(target_os = "linux")]
use crate::linux::factory as imp;
#[cfg(target_os = "macos")]
use crate::macos::factory as imp;
#[cfg(windows)]
use crate::windows::factory as imp;
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
use unsupported as imp;

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod unsupported;

use crate::{InputError, InputListener, KeyEmitter, KeyGate};

/// A gate that can hold the user's keystrokes back while a correction
/// is being typed, paired with the listener [`create_listener`]
/// returns: on Linux/evdev the two share the device thread that owns
/// the grabs, so **create the gate first and pass it in**. Every other
/// backend returns a no-op gate.
///
/// Whether it can actually hold anything is only known once the
/// listener has started — see [`KeyGate::available`].
///
/// `hold_keys` is the `[engine].hold_keys` config value. Wired to the
/// macOS gate only for now: the Windows gate keeps its env-only switch
/// until the change can go through a Windows test run, and Linux/evdev
/// holds by construction.
pub fn create_key_gate(hold_keys: bool) -> KeyGate {
    imp::create_key_gate(hold_keys)
}

/// Construct the listener appropriate for the current OS, wired to
/// `gate` where the backend supports one.
pub fn create_listener(gate: &KeyGate) -> Result<Box<dyn InputListener>, InputError> {
    imp::create_listener(gate)
}

pub fn create_emitter() -> Result<Box<dyn KeyEmitter>, InputError> {
    imp::create_emitter()
}
