//! Tray icon rendering: a 16×16 RGBA icon showing a 2-letter layout
//! code on a tinted background.
//!
//! The font is a built-in 4×6-pixel bitmap covering A–Z and 0–9 — ~340
//! bytes, small enough to embed inline and big enough to read at 16×16.
//! No font crate and no `tiny-skia`, so the icon can be redrawn on
//! every layout change without GPU round-trips.

mod consts;
mod render;

pub(crate) use consts::*;
pub use render::*;

#[cfg(test)]
mod tests;
