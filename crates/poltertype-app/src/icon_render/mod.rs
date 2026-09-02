//! Tray icon rendering: a 2-letter layout code on a tinted background,
//! on nothing at all in `mono`, or the layout's country flag in
//! `flag`.
//!
//! Everything is drawn on a 16-unit design grid and emitted at
//! [`SCALE`]× that, so a panel scales the icon *down* rather than up;
//! an upscaled 16×16 bitmap is what issue #54 photographed. The font is
//! a built-in 4×6-unit bitmap covering A–Z and 0–9 — ~340 bytes, small
//! enough to embed inline. No font crate and no `tiny-skia`, so the
//! icon can be redrawn on every layout change without GPU round-trips.

mod consts;
pub(crate) mod flag;
mod render;

pub(crate) use consts::*;
pub use render::*;

#[cfg(test)]
mod tests;
