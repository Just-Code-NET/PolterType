//! Cinnamon layout switcher.
//!
//! Cinnamon needs its own backend because it looks like GNOME from the
//! outside and behaves like nothing else inside. It ships and populates
//! `org.gnome.desktop.input-sources`, so the gsettings backend claimed
//! the session on sight — and every write went into dconf and no
//! further. The layout never moved, and the next `current()` read our
//! own write back and told the engine the switch had happened, so
//! corrections stopped firing altogether
//! ([#26](https://github.com/Just-Code-NET/PolterType/issues/26)).
//!
//! Its own fork of the schema is no better: only `sources` is live, and
//! nothing in Cinnamon reads or writes the `current` key it also
//! declares — the active source is in-memory state of
//! `InputSourceManager`. So the two real ways in are version-dependent:
//!
//! * **6.6+** exposes `org.Cinnamon.GetInputSources()` and
//!   `ActivateInputSourceIndex(i)` on the session bus — the same entry
//!   point the keyboard applet uses, so the indicator follows.
//! * **6.4 and older** has no such API. There the applet drives
//!   `XAppKbdLayoutController`, which is libgnomekbd's
//!   `gkbd_configuration_lock_group()`, which is plain `XkbLockGroup`.
//!   Layouts are ordinary XKB groups and the controller listens for
//!   group changes, so locking one both switches the keyboard and
//!   updates the indicator — exactly what our X11 backend does, which
//!   is why this routes there explicitly rather than by falling through.
//!
//! Which applies is decided by *asking*: if `GetInputSources` answers,
//! this is 6.6+.
//!
//! IBus is a red herring despite `GTK_IM_MODULE=ibus`. Cinnamon
//! activates an IBus engine on every switch only so XIM clients keep
//! working, and the engines it picks are the `xkb:…` ones which, in the
//! words of Cinnamon's own `keyboardManager.js`, "simply 'echo' back
//! symbols, despite their naming implying differently".

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
