//! Keeping a tray-only app out of the Dock and the app switcher.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
use other as imp;

/// Keep a tray-only app out of the Dock and the app switcher.
///
/// `LSUIElement` in the bundle's `Info.plist` is not enough on its own:
/// `tao` applies `ActivationPolicy::Regular` at startup and overrides
/// it. Must be called before the event loop runs — afterwards the
/// policy is fixed for the process.
pub use imp::keep_out_of_dock;
