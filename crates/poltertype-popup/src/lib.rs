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
//!   window (never focused by the WM). Windows:
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
//! | Wayland — wlroots (Hyprland, Sway, …) **and KWin** | layer-shell | primary target; KWin verified 6.7.3 |
//! | X11 | override-redirect window | zero-permission path |
//! | GNOME Wayland | override-redirect via XWayland | Mutter has no layer-shell; the X11 fallback still maps |
//! | Wayland with neither layer-shell nor XWayland | noop | the only remaining gap |
//! | Windows 10 / 11 | layered topmost window | `UpdateLayeredWindow`; per-monitor DPI |
//! | macOS | noop today | seam ready; see `docs/PLAN.md` |
//!
//! The backends are *probed*, not selected from a table of desktop
//! names: layer-shell first, X11 second, noop last. That is why KDE
//! worked the whole time nobody claimed it did — and why a compositor
//! that gains layer-shell tomorrow needs no code change here.
//!
//! This crate is one of the platform-code islands (see the workspace
//! `CLAUDE.md` hard rules): `#[cfg(target_os)]` is allowed here and
//! nowhere outside `poltertype-input` / `poltertype-layout` /
//! `poltertype-update` / this crate.

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
