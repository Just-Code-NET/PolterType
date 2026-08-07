//! Cinnamon layout switcher.
//!
//! Cinnamon needs a backend of its own because it looks like GNOME
//! from the outside and behaves like nothing else from the inside. It
//! ships `org.gnome.desktop.input-sources` (the schema comes with the
//! shared GTK stack) and populates it, so the gsettings backend used
//! to claim the session on sight — and then every `gsettings set …
//! current` we wrote went into dconf and no further. The layout never
//! moved, the applet never moved, and the next `current()` read our
//! own write back and told the engine the switch had happened, so
//! corrections stopped firing altogether. Reported from Linux Mint 22
//! ([#26](https://github.com/Just-Code-NET/PolterType/issues/26)).
//!
//! Cinnamon does keep a fork of that schema —
//! `org.cinnamon.desktop.input-sources` — but only its `sources` key
//! is live. The schema also declares `current`, and nothing in
//! Cinnamon reads or writes it; the active source is in-memory state
//! of `InputSourceManager`. So neither schema is a way in, and the
//! two real ways in are version-dependent:
//!
//! * **Cinnamon 6.6+** exposes the input sources on the session bus:
//!   `org.Cinnamon.GetInputSources()` and
//!   `org.Cinnamon.ActivateInputSourceIndex(i)`. That is the same
//!   entry point the keyboard applet uses, so the indicator follows.
//! * **Cinnamon 6.4 and older** (Linux Mint 22.x) has no such API.
//!   There the applet drives `XAppKbdLayoutController`, which is
//!   libgnomekbd's `gkbd_configuration_lock_group()`, which is plain
//!   `XkbLockGroup`. Layouts are ordinary XKB groups, and the
//!   controller listens for group changes, so locking a group both
//!   switches the keyboard and updates the indicator. That is exactly
//!   what our X11 backend does — it is simply probed last, for
//!   sessions where no desktop owns the layout, and Cinnamon has to
//!   route to it explicitly.
//!
//! Which of the two applies is decided by *asking*, not by parsing a
//! version string: if `GetInputSources` answers, this is 6.6+.
//!
//! IBus is a red herring here even though Cinnamon sets
//! `GTK_IM_MODULE=ibus`. Cinnamon activates an IBus engine on every
//! switch, but only so XIM clients keep working — the engines it
//! picks are the `xkb:…` ones, and, in the words of the comment above
//! the call in Cinnamon's own `keyboardManager.js`, those "simply
//! 'echo' back symbols, despite their naming implying differently".
//! Driving `ibus engine` here would be a second write that changes no
//! layout.

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod dbus;
mod session;
mod switcher;

pub use consts::*;
pub(crate) use dbus::*;
pub(crate) use session::*;
pub use switcher::*;

#[cfg(test)]
mod tests;
