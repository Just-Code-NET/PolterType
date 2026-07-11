//! Tray icon rendering.
//!
//! Renders a 16×16 RGBA icon showing a 2-letter layout code (e.g.
//! `EN`, `UK`) on a tinted background. The font is a tiny built-in
//! 4×6-pixel bitmap covering A–Z + 0–9 — small enough to embed
//! inline (~340 bytes), big enough to read at 16×16 in the system
//! tray.
//!
//! No font crate, no `tiny-skia` dep — keeps the binary lean and
//! lets the tray icon update on every layout change without GPU
//! roundtrips.

mod consts;
mod render;

pub(crate) use consts::*;
pub use render::*;

#[cfg(test)]
mod tests;
