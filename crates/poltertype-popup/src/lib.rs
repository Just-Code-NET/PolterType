//! The suggestion tooltip: a small overlay near the focused window
//! showing spelling suggestions for a mistyped word. Click an entry
//! (or press the accept chord + digit — handled by the engine, not
//! here) to replace the word.
//!
//! ## Hard requirements every backend must honour
//!
//! * **Never take keyboard focus.** The user is mid-typing; a popup
//!   that grabs focus breaks the very keystrokes we exist to fix.
//!   Wayland: `wlr-layer-shell` surface with
//!   `keyboard_interactivity = None`. X11: an override-redirect
//!   window (never focused by the WM). Windows (future):
//!   `WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW`.
//! * **Never log the words being shown** — same privacy rule as the
//!   engine: typed text stays out of the logs at any level.
//! * **The popup thread never blocks the caller.** `show` / `hide`
//!   enqueue and return; all OS I/O happens on the popup's own
//!   thread.
//!
//! ## Platform coverage
//!
//! | Platform | Backend | Notes |
//! |---|---|---|
//! | Wayland (wlroots: Hyprland, Sway, …) | layer-shell | primary target |
//! | X11 | override-redirect window | zero-permission path |
//! | GNOME/KDE Wayland | noop | no layer-shell for 3rd-party apps |
//! | Windows / macOS | noop today | seam ready; see `docs/PLAN.md` |
//!
//! This crate is one of the platform-code islands (see the workspace
//! `CLAUDE.md` hard rules): `#[cfg(target_os)]` is allowed here and
//! nowhere outside `poltertype-input` / `poltertype-layout` /
//! `poltertype-update` / this crate.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod enums;
mod factory;
mod noop;
// The shared placement + renderer are consumed only by the Linux
// backends today. On Windows/macOS (noop-only) compiling them trips
// `-D dead_code` on the CI lanes — un-gate these (and `tests`) when
// a backend lands there.
#[cfg(target_os = "linux")]
mod place;
#[cfg(target_os = "linux")]
mod render;
mod traits;
mod types;

#[cfg(target_os = "linux")]
mod linux;

pub use enums::{PopupAnchor, PopupUiEvent};
pub use factory::create_popup;
pub use traits::SuggestionPopup;
pub use types::{PopupEntry, PopupModel};

#[cfg(all(test, target_os = "linux"))]
mod tests;
