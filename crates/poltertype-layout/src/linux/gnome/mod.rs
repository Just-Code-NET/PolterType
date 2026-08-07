//! GSettings-based layout switcher.
//!
//! Despite the file name, this covers every DE that drives layouts
//! through `org.gnome.desktop.input-sources`: **GNOME**, **Ubuntu
//! Unity 7+**, **Budgie**, **Pantheon** (elementary OS).
//!
//! The schema's `sources` is an array of `(type, id)` pairs (typically
//! `('xkb', 'us')` etc.) and `current` is a `u` index into it.
//! Switching = writing a new `current`.
//!
//! `try_init()` is a strict probe, and both of its conditions come
//! from a session where the schema was present and writing it did
//! nothing:
//!
//! * `sources` must read back **populated**. The schema ships with
//!   GTK, so a bare i3 or openbox session with one GTK app installed
//!   has it too — and reads back an empty list. Claiming the session
//!   there would shadow the X11 backend that does work.
//! * the desktop must not be one **known to ignore the schema**.
//!   Cinnamon installs and populates it while driving the layout from
//!   somewhere else entirely
//!   ([#26](https://github.com/Just-Code-NET/PolterType/issues/26));
//!   it has its own backend, and this one stands down for it. See
//!   [`crate::linux::cinnamon`] for what Cinnamon actually reads.
//!
//! So KDE / Hyprland / Cinnamon / IBus-only / Fcitx-only sessions all
//! fall through to their own backends instead of being claimed here.

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod enums;
mod gsettings;
mod switcher;

pub use consts::*;
pub(crate) use enums::*;
pub use gsettings::*;
pub use switcher::*;
