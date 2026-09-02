//! The tray icon as the layout's country flag.
//!
//! Same rasteriser as the lettered badge — a 4×6 bitmap font's worth
//! of arithmetic, no font crate and no `tiny-skia` — so the flag can
//! be redrawn on every layout change. Each drawing is a function of
//! one point rather than a stored image, which is what keeps this a
//! repository with no binary assets in it.
//!
//! Only countries whose flag survives being 64×48 pixels are here.
//! `table` says which, and why the ones that are missing are missing.

mod consts;
mod paint;
mod region;
mod render;
mod table;

pub(crate) use render::render;

#[cfg(test)]
mod tests;
