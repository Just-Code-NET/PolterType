//! Linux backends: Wayland layer-shell (primary) and X11
//! override-redirect (zero-permission fallback).

mod probe;
mod wayland;
mod x11;

pub(crate) use probe::create_for_platform;
