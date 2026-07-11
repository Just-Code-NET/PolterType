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
//! `try_init()` is a strict probe — it requires both `gsettings` in
//! `$PATH` *and* a successful read of `sources` from the schema. So
//! KDE / Hyprland / IBus / Fcitx-only sessions correctly fall through
//! to their own backends instead of being claimed here.

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod gsettings;
mod switcher;

pub use consts::*;
pub use gsettings::*;
pub use switcher::*;
