//! `KeyGate` — the "hold the user's keystrokes back while we type" seam.

#[cfg(target_os = "linux")]
use crate::linux::wayland::EvdevGate as Backend;
#[cfg(target_os = "macos")]
use crate::macos::MacosGate as Backend;
#[cfg(windows)]
use crate::windows::WindowsGate as Backend;
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
use disabled::DisabledGate as Backend;

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod disabled;

use std::sync::Arc;

/// Holds physical keystrokes back from applications for the duration of
/// a correction burst, then lets them through again.
///
/// The only real answer to keystrokes scrambling a correction:
/// everything we inject travels the same path to the compositor as
/// everything the user types, so a key pressed mid-burst lands *inside*
/// our text and no counting afterwards can put it back. Held keys still
/// reach the engine, which replays them behind the correction in typed
/// order.
///
/// A gate reporting `available() == false` is a no-op, and that is the
/// common case. Callers must treat [`hold`](Self::hold) returning
/// `false` as normal and stay correct without it.
#[derive(Clone, Default)]
pub struct KeyGate {
    inner: Option<Arc<Backend>>,
}

impl KeyGate {
    /// A gate that does nothing — the default on platforms without an
    /// implementation, and what tests use.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Wrap an already-built per-OS backend. Every backend answers the
    /// same `available` / `hold` / `release` shape — duck-typed rather
    /// than a trait, since the Linux device thread's `service<D:
    /// GateDevice>` needs a concrete device type a `dyn` backend would
    /// lose.
    pub(crate) fn with_backend(inner: Arc<Backend>) -> Self {
        Self { inner: Some(inner) }
    }

    pub(crate) fn backend(&self) -> Option<&Arc<Backend>> {
        self.inner.as_ref()
    }

    /// Can this gate actually hold keys? Answered by the backend once
    /// the input stack is up, so it is only meaningful after the
    /// listener has started.
    pub fn available(&self) -> bool {
        self.inner.as_ref().is_some_and(|g| g.available())
    }

    /// Hold the user's keystrokes back. Returns whether the hold is
    /// actually in force — `false` means carry on unprotected.
    ///
    /// Every hold must be paired with [`release`](Self::release), but
    /// the backend also enforces its own ceiling: a caller that dies
    /// mid-correction cannot leave the keyboard dead.
    pub fn hold(&self) -> bool {
        self.inner.as_ref().is_some_and(|g| g.hold())
    }

    /// Let the user's keystrokes through again. Idempotent.
    pub fn release(&self) {
        if let Some(g) = self.inner.as_ref() {
            g.release();
        }
    }
}

impl std::fmt::Debug for KeyGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyGate")
            .field("available", &self.available())
            .finish()
    }
}
