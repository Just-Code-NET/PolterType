//! Typing through the `org.freedesktop.portal.RemoteDesktop` portal.
//!
//! **Never executed.** There is no RemoteDesktop backend on the machine
//! this was written on, so every line is written from the portal
//! specification and has run exactly zero times. If it misbehaves on a
//! real GNOME or KDE session, start by assuming this file is wrong
//! rather than the compositor.
//!
//! It exists because `uinput` — the only other way to type on Wayland —
//! needs `input`-group membership plus a udev rule, one `sudo` before
//! the app does anything. The portal is the standard, permissioned way
//! to ask the compositor to synthesise input, so it is the one path
//! that could remove that step.
//!
//! **Not libei**, deliberately. The portal already exposes
//! [`NotifyKeyboardKeycode`][spec] as a plain D-Bus method, which does
//! exactly what a correction needs; `ConnectToEIS` would mean a new
//! protocol implementation and a heavyweight dependency to send twenty
//! keystrokes, and would still need everything in [`session`] to get
//! the descriptor.
//!
//! Three limits: **no `send_text`** (the portal speaks keycodes, which
//! suits the Wayland path anyway); **it asks** — `Start` shows a
//! consent dialog, and a restore token only silences later sessions;
//! and **no echo suppression of its own**, so the emitter records what
//! it sent for the engine's echo filter, as uinput does.
//!
//! [spec]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html

mod consts;
mod emitter;
mod enums;
mod response;
mod restore_token;
mod session;

pub use emitter::PortalEmitter;
pub use enums::PortalError;
pub use session::{PortalSession, portal_available};

#[cfg(test)]
mod tests;
