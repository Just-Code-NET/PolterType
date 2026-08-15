//! The PolterType app icon, drawn procedurally.
//!
//! The repo intentionally ships **no** binary asset for the app icon.
//! It is a transcription of the site's brand mark
//! (`poltertype-web/public/favicon.svg`) authored in the same 64-unit
//! square and rebuilt out of predicates rather than paths: a rounded
//! rectangle, a half-ellipse, some discs and one stroked arc, which is
//! cheaper than dragging an SVG renderer into the build for one image.
//!
//! **The two marks have to be edited together.** Nothing checks that
//! they match, so a favicon change that stops there leaves the app
//! wearing last season's logo — exactly how the icon went on reading
//! `kb` long after the rename.
//!
//! Five consumers, one geometry:
//!
//! * `poltertype-app`'s build script embeds [`render_ico`]'s output as
//!   the executable's icon resource — what Explorer, the Start menu,
//!   the taskbar and Alt-Tab all read.
//! * `cargo xtask assets icon-ico` writes the same `.ico` for the MSI's
//!   Add/Remove Programs entry.
//! * `cargo xtask assets icon-png` writes a PNG that release CI turns
//!   into `.icns` for macOS and uses as-is for the AppImage.
//! * The Settings window builds its own icon from [`rasterise`] when it
//!   opens.
//! * `poltertype-shell` renders [`render_png`] into the user's
//!   `hicolor` theme on Linux, where an app's icon lives in a shared
//!   directory rather than in the executable.
//!
//! The last two make this a **runtime** dependency of the app as well
//! as a build-time one — it was build-time only until v0.17.1 gave the
//! Settings window an icon of its own.
//!
//! Swapping in a hand-designed image later is "delete `consts`,
//! `shapes` and `render`, and decode a checked-in PNG instead" — the
//! two output formats and every caller stay put.

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
