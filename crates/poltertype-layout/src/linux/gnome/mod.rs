//! GSettings-based layout switcher.
//!
//! Despite the file name, this covers every DE that exposes the
//! `org.gnome.desktop.input-sources` schema, which is the lingua
//! franca for GNOME-derivative environments: **GNOME**, **Ubuntu
//! Unity 7+**, **Cinnamon**, **Budgie**, **Pantheon** (elementary
//! OS), **MATE** (when configured via gsettings).
//!
//! The schema's `sources` is an array of `(type, id)` pairs (typically
//! `('xkb', 'us')` etc.) and `current` is a `u` index into it.
//! Switching = writing a new `current`.
//!
//! `try_init()` is a strict probe — it requires `gsettings` in
//! `$PATH`, a successful read of a populated `sources` from the
//! schema, *and* that nothing else is mediating input on this session
//! (see [`probe`]). So KDE / Hyprland / IBus / Fcitx-only sessions
//! correctly fall through to their own backends instead of being
//! claimed here.

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod enums;
mod gsettings;
mod probe;
mod switcher;

pub use consts::*;
pub(crate) use enums::*;
pub use gsettings::*;
pub(crate) use probe::*;
pub use switcher::*;

#[cfg(test)]
mod tests;
