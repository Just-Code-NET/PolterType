//! Typing through the `org.freedesktop.portal.RemoteDesktop` portal.
//!
//! **Never executed.** There is no RemoteDesktop backend on the
//! machine this was written on — `hyprland.portal` offers Screenshot,
//! ScreenCast, GlobalShortcuts and InputCapture, and only
//! `kde.portal` declares RemoteDesktop — so every line here is
//! written from the portal specification and has run exactly zero
//! times. Treat it the way `CLAUDE.md` tells you to treat the macOS
//! paths. If it misbehaves on a real GNOME or KDE session, start by
//! assuming this file is wrong rather than the compositor.
//!
//! ## Why this exists
//!
//! `uinput` is the only way PolterType can type on Wayland, and it
//! needs `input`-group membership plus a udev rule — one `sudo` the
//! user has to run before the app does anything. X11 needs nothing;
//! Wayland needs a setup script. The portal is the standard,
//! permissioned way to ask the compositor to synthesise input, so it
//! is the one path that could remove that step for GNOME and KDE.
//!
//! ## Why not libei
//!
//! The plan said "`libei` (`reis`) as the portal variant of
//! send-keys", and libei is indeed where this is all heading. But the
//! portal already exposes [`NotifyKeyboardKeycode`][spec] as a plain
//! D-Bus method, which does exactly what a correction needs: press
//! and release an evdev keycode. Going through `ConnectToEIS` and the
//! libei protocol instead would mean a new protocol implementation
//! and a heavyweight dependency to send perhaps twenty keystrokes per
//! correction, and it would still need everything in
//! [`session`] to get the file descriptor in the first place.
//!
//! So this takes the short path deliberately. `zbus` is already in
//! the tree for the a11y bus, the whole backend is a few hundred
//! reviewable lines, and if a compositor ever drops the Notify
//! methods in favour of EIS-only, [`session`] is the part that
//! already exists and `emitter` is what gets replaced.
//!
//! ## What it cannot do
//!
//! * **No `send_text`.** The portal speaks keycodes, not characters.
//!   That is fine — the engine's Wayland path is scancode replay
//!   anyway, for exactly the same reason (see `KeyEmitter::send_keys`).
//! * **It asks.** `Start` shows a consent dialog. A restore token is
//!   requested so that later sessions are silent, but the first run
//!   on any machine puts a dialog in front of the user, and a
//!   compositor is free to ask again.
//! * **No echo suppression of its own.** Injected keys come back
//!   through evdev like anything else, so the emitter records what it
//!   sent for the engine's echo filter, exactly as the uinput backend
//!   does.
//!
//! [spec]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html

mod consts;
mod emitter;
mod session;

pub use emitter::PortalEmitter;
pub use session::{PortalError, PortalSession, portal_available};

#[cfg(test)]
mod tests;
