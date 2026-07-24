//! Linux backends: Wayland layer-shell (primary) and X11
//! override-redirect (zero-permission fallback).

pub(crate) mod wayland;
pub(crate) mod x11;
