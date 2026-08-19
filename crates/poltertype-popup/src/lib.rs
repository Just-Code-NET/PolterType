//! The suggestion tooltip: a small overlay near the focused window
//! showing spelling suggestions for a mistyped word.
//!
//! Three hard requirements every backend must honour:
//!
//! * **Never take keyboard focus.** The user is mid-typing, and a popup
//!   that grabs focus breaks the very keystrokes we exist to fix.
//!   Wayland uses a layer-shell surface with
//!   `keyboard_interactivity = None`, X11 an override-redirect window,
//!   Windows `WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW`,
//!   macOS a borderless `NSPanel` with `NonactivatingPanel`.
//! * **Never log the words being shown**, same rule as the engine.
//! * **Never block the caller.** `show` / `hide` enqueue and return;
//!   all OS I/O happens on the popup's own thread (Linux/Windows) or
//!   on the main dispatch queue (macOS — AppKit's rule; see
//!   `docs/MACOS_POPUP.md`).
//!
//! Backends are *probed*, not chosen from a table of desktop names:
//! layer-shell, then X11, then noop. That is why KDE worked the whole
//! time nobody claimed it did. Current coverage lives in the README
//! rather than this header, so it cannot go stale twice.
//!
//! One of the platform-code islands — `#[cfg(target_os)]` is allowed
//! here; see `CONTRIBUTING.md`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod enums;
mod factory;
// The fallback for platforms and probes without an overlay path;
// unused on macOS, where the one backend is infallible.
#[cfg(not(target_os = "macos"))]
mod noop;
// Gated because a platform with no backend compiling them would trip
// `-D dead_code` on that CI lane; add new targets here as backends
// land.
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
mod place;
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
mod render;
mod traits;
mod types;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub use enums::{PopupAnchor, PopupUiEvent};
pub use factory::create_popup;
pub use traits::SuggestionPopup;
pub use types::{PopupEntry, PopupModel};

#[cfg(all(test, any(target_os = "linux", windows, target_os = "macos")))]
mod tests;
