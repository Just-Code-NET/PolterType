//! The suggestion tooltip: a small overlay near the focused window
//! showing spelling suggestions for a mistyped word.
//!
//! Three hard requirements every backend must honour:
//!
//! * **Never take keyboard focus.** The user is mid-typing, and a popup
//!   that grabs focus breaks the very keystrokes we exist to fix.
//!   Wayland uses a layer-shell surface with
//!   `keyboard_interactivity = None`, X11 an override-redirect window,
//!   Windows `WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW`.
//! * **Never log the words being shown**, same rule as the engine.
//! * **Never block the caller.** `show` / `hide` enqueue and return;
//!   all OS I/O happens on the popup's own thread.
//!
//! Backends are *probed*, not chosen from a table of desktop names:
//! layer-shell, then X11, then noop. That is why KDE worked the whole
//! time nobody claimed it did, and why a compositor that gains
//! layer-shell tomorrow needs no change here. Current coverage lives in
//! the README rather than this header, so it cannot go stale twice.
//!
//! One of the platform-code islands — `#[cfg(target_os)]` is allowed
//! here; see `CONTRIBUTING.md`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod enums;
mod factory;
mod noop;
// The shared placement + renderer are consumed by every real backend.
// Still gated, because macOS has none and compiling them there would
// trip `-D dead_code` on that CI lane; add its target here when one
// lands.
#[cfg(any(target_os = "linux", windows))]
mod place;
#[cfg(any(target_os = "linux", windows))]
mod render;
mod traits;
mod types;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

pub use enums::{PopupAnchor, PopupUiEvent};
pub use factory::create_popup;
pub use traits::SuggestionPopup;
pub use types::{PopupEntry, PopupModel};

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests;
