//! Procedural icon generation for the release installers.
//!
//! The repo intentionally ships **no** binary asset for the app icon
//! — `icon_render` already draws the *tray* icon (dynamic, layout-
//! coded, 16×16) procedurally, and the installers just need a static
//! square placeholder until someone designs a real brand mark. So we
//! render a 1024×1024 RGBA PNG at packaging time:
//!
//! * **Background** — indigo rounded square (12% corner radius), with
//!   a 1.5px AA band so the corners don't look jagged at large sizes.
//! * **Foreground** — the wordmark `kb` in white, drawn from a tiny
//!   inlined 5×7 bitmap font scaled with nearest-neighbour. Two
//!   glyphs is enough to identify the app; doing a real text
//!   renderer would mean dragging in a font crate for two letters.
//!
//! The CI then runs platform-specific tools to convert this PNG into
//! `.ico` (Windows / `magick`), `.icns` (macOS / `sips` + `iconutil`),
//! and uses it as-is for the AppImage on Linux.
//!
//! Replacing the procedural icon with a hand-designed PNG is just
//! "delete this module and check `assets/icon-1024.png` into the
//! repo" — the consumers (`installers/*/build-*.sh|.ps1`) already
//! treat the path as an opaque input.
//!
//! Why no `image` crate dep: this module needs **one** PNG with **one**
//! pixel format (RGBA8). The `png` crate is ~50 KB; `image` would
//! pull in JPEG, GIF, TIFF, BMP decoders we'll never use.

mod consts;
mod render;

pub(crate) use consts::*;
pub use render::*;
