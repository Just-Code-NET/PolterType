//! Procedural app-icon generation for the release installers.
//!
//! The repo intentionally ships **no** binary asset for the app icon:
//! `icon_render` already draws the tray icon procedurally, so the
//! installers' static square is drawn the same way — a 1024×1024 RGBA
//! PNG rendered at packaging time.
//!
//! It draws the PolterType mark, a transcription of the site's brand
//! mark (`poltertype-web/public/favicon.svg`) authored in the same
//! 64-unit square and rebuilt out of predicates rather than paths: a
//! rounded rectangle, a half-ellipse, some discs and one stroked arc,
//! which is cheaper than dragging an SVG renderer into the build for
//! one image.
//!
//! **The two marks have to be edited together.** Nothing checks that
//! they match, so a favicon change that stops there leaves the app
//! wearing last season's logo — exactly how the icon went on reading
//! `kb` long after the rename.
//!
//! CI converts the PNG to `.ico` and `.icns` and uses it as-is for the
//! AppImage. Swapping in a hand-designed PNG later is "delete this
//! module and check `assets/icon-1024.png` in" — the installer scripts
//! already treat the path as an opaque input.
//!
//! No `image` dependency: this needs one PNG in one pixel format, and
//! `image` would pull in decoders we will never use.

mod consts;
mod render;
mod shapes;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use consts::*;
pub use render::*;
pub(crate) use shapes::*;
pub(crate) use types::*;
