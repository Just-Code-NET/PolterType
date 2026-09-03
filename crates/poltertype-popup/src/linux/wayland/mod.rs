//! Wayland backend: a `wlr-layer-shell` overlay surface with
//! `keyboard_interactivity = None`, so the popup can never steal the
//! keys it exists to fix. Works on wlroots compositors **and on KWin**,
//! which has implemented `zwlr_layer_shell_v1` for years. Mutter is the
//! holdout (GNOME/mutter#973), detected at connect time so the factory
//! falls through to X11/noop.
//!
//! All Wayland state lives on one dedicated thread; the public handle
//! only pushes commands into a channel. The thread parks on that
//! channel while hidden and ticks at ~16 ms while a surface is mapped,
//! pumping the queue manually.

mod enums;
mod handlers;
mod popup;
mod run;
mod state;
mod view;

pub(crate) use popup::WaylandPopup;
