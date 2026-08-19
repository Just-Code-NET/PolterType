//! The PolterType app icon, drawn procedurally.
//!
//! The repo intentionally ships **no** binary asset for the app icon.
//! It is a transcription of the site's brand mark
//! (`poltertype-web/public/favicon.svg`) authored in the same 64-unit
//! square and rebuilt out of predicates rather than paths, which is
//! cheaper than dragging an SVG renderer into the build for one image.
//!
//! **The two marks have to be edited together.** Nothing checks that
//! they match, so a favicon change that stops there leaves the app
//! wearing last season's logo — exactly how the icon went on reading
//! `kb` long after the rename.
//!
//! One geometry, five consumers: the executable's Windows icon
//! resource and the MSI's ([`render_ico`]), the PNG release CI turns
//! into `.icns` and reuses for the AppImage ([`render_png`]), the
//! Settings window's own icon ([`rasterise`]), and the `hicolor` theme
//! `poltertype-shell` writes on Linux. The last two make this a
//! **runtime** dependency of the app, not only a build-time one.

mod consts;
mod enums;
mod ico;
mod render;
mod shapes;
mod types;

#[cfg(test)]
mod tests;

pub use consts::{ICO_SIZES, MIN_PNG_SIZE};
pub use enums::IconError;
pub use ico::render_ico;
pub use render::{rasterise, render_png};

pub(crate) use consts::*;
pub(crate) use render::{encode_png, write_file};
pub(crate) use shapes::*;
pub(crate) use types::*;
