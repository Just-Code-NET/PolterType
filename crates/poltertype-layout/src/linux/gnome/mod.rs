//! GSettings-based layout switcher.
//!
//! Despite the file name, this covers every desktop that drives layouts
//! through `org.gnome.desktop.input-sources`: GNOME, Ubuntu Unity 7+,
//! Budgie and Pantheon. `sources` is an array of `(type, id)` pairs and
//! `current` is a `u` index into it, so switching is a write to
//! `current`.
//!
//! `try_init()` is a strict probe, and both conditions come from a
//! session where the schema was present and writing it did nothing:
//!
//! * `sources` must read back **populated**. The schema ships with GTK,
//!   so a bare i3 session with one GTK app installed has it too and
//!   reads back empty — claiming the session there would shadow the X11
//!   backend that does work.
//! * the desktop must not be one **known to ignore the schema**.
//!   Cinnamon installs and populates it while driving the layout from
//!   somewhere else entirely
//!   ([#26](https://github.com/Just-Code-NET/PolterType/issues/26)), so
//!   this backend stands down for it — see [`crate::linux::cinnamon`].
//!   The wlroots compositors (labwc, sway, river, …) are the same case
//!   measured on a second desktop: they keep their own xkb config, and
//!   the write reports success while the keyboard never changes group.

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod enums;
mod gsettings;
mod switcher;

pub use consts::*;
pub(crate) use enums::*;
pub use gsettings::*;
pub use switcher::*;

#[cfg(test)]
mod tests;
