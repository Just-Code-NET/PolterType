//! `KeyGate` — the "hold the user's keystrokes back while we type" seam.

// Only the evdev, Windows and macOS backends have anything behind the
// gate; elsewhere `KeyGate` is an empty struct and this import would
// be dead.
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
use std::sync::Arc;

/// Holds physical keystrokes back from applications for the duration of
/// a correction burst, then lets them through again.
///
/// This is the only real answer to keystrokes scrambling a correction.
/// Everything we inject travels the same path to the compositor as
/// everything the user types, so a key pressed while a burst is on the
/// wire lands *inside* our text (`зтзь ш ` coming out as `ipnpm `), and
/// no amount of counting afterwards can put it back where it belongs.
/// Held keys are still delivered to the engine, which replays them
/// behind the correction in the order they were typed.
///
/// A gate that reports `available() == false` is a no-op, and that is
/// the common case: the Windows one is off unless
/// `POLTERTYPE_HOLD_KEYS=1` because it has never run on real hardware,
/// and even the evdev gate stands down on stacks where it would do
/// more harm than good. Callers must therefore treat
/// [`hold`](Self::hold) returning `false` as normal and stay correct
/// without it.
#[derive(Clone, Default)]
pub struct KeyGate {
    #[cfg(target_os = "linux")]
    inner: Option<Arc<crate::linux::wayland::EvdevGate>>,
    #[cfg(windows)]
    inner: Option<Arc<crate::windows::WindowsGate>>,
    #[cfg(target_os = "macos")]
    inner: Option<Arc<crate::macos::MacosGate>>,
}

impl KeyGate {
    /// A gate that does nothing — the default on platforms without an
    /// implementation, and what tests use.
    pub fn disabled() -> Self {
        Self::default()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn evdev(inner: Arc<crate::linux::wayland::EvdevGate>) -> Self {
        Self { inner: Some(inner) }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn evdev_inner(&self) -> Option<&Arc<crate::linux::wayland::EvdevGate>> {
        self.inner.as_ref()
    }

    #[cfg(windows)]
    pub(crate) fn windows(inner: Arc<crate::windows::WindowsGate>) -> Self {
        Self { inner: Some(inner) }
    }

    #[cfg(windows)]
    pub(crate) fn windows_inner(&self) -> Option<&Arc<crate::windows::WindowsGate>> {
        self.inner.as_ref()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn macos(inner: Arc<crate::macos::MacosGate>) -> Self {
        Self { inner: Some(inner) }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn macos_inner(&self) -> Option<&Arc<crate::macos::MacosGate>> {
        self.inner.as_ref()
    }

    /// Can this gate actually hold keys? Answered by the backend once
    /// the input stack is up, so it is only meaningful after the
    /// listener has started.
    pub fn available(&self) -> bool {
        #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
        {
            self.inner.as_ref().is_some_and(|g| g.available())
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            false
        }
    }

    /// Hold the user's keystrokes back. Returns whether the hold is
    /// actually in force — `false` means carry on unprotected.
    ///
    /// Every hold must be paired with [`release`](Self::release), but
    /// the backend also enforces its own ceiling: a caller that dies
    /// mid-correction cannot leave the keyboard dead.
    pub fn hold(&self) -> bool {
        #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
        {
            self.inner.as_ref().is_some_and(|g| g.hold())
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            false
        }
    }

    /// Let the user's keystrokes through again. Idempotent.
    pub fn release(&self) {
        #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
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
