//! Procedural app-icon generation for the release installers.
//!
//! The repo intentionally ships **no** binary asset for the app icon —
//! `icon_render` already draws the *tray* icon (dynamic, layout-coded,
//! 16×16) procedurally, so the installers' static square is drawn the
//! same way: a 1024×1024 RGBA PNG rendered at packaging time.
//!
//! What it draws is the PolterType mark — the ghost on an indigo
//! keycap. It is a transcription of the site's brand mark
//! (`poltertype-web/public/favicon.svg`), authored in the same 64-unit
//! square and rebuilt out of predicates instead of paths: a rounded
//! rectangle, a half-ellipse, some discs and one stroked arc are the
//! whole picture, which is cheaper than dragging an SVG renderer into
//! the build for one image. `consts` holds the geometry, `shapes` the
//! predicates, `render` the sampler.
//!
//! **The two marks have to be edited together.** Nothing checks that
//! they match, so a change to the favicon that stops here leaves the
//! app wearing last season's logo — which is exactly how the icon
//! ended up reading `kb` long after the rename.
//!
//! The CI then runs platform-specific tools to convert this PNG into
//! `.ico` (Windows / `magick`), `.icns` (macOS / `sips` + `iconutil`),
//! and uses it as-is for the AppImage on Linux.
//!
//! Swapping in a hand-designed PNG later is just "delete this module
//! and check `assets/icon-1024.png` into the repo" — the consumers
//! (`installers/*/build-*.sh|.ps1`) already treat the path as an
//! opaque input.
//!
//! Why no `image` crate dep: this module needs **one** PNG with **one**
//! pixel format (RGBA8). The `png` crate is ~50 KB; `image` would
//! pull in JPEG, GIF, TIFF, BMP decoders we'll never use.

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
