//! Constants named by more than one module.

/// The id this app's windows carry, the stem of its `.desktop` entry
/// and its icon name inside `hicolor` — one string, so a window, a
/// menu entry and a notification cannot disagree.
///
/// Deliberately **not** `APP_ID` (`dev.opensource.poltertype`), which
/// names the autostart entry and the instance lock: the AppImage and
/// the AUR `PKGBUILD` have shipped `poltertype.desktop` since before
/// this module existed, and renaming their file would fix nothing.
pub const DESKTOP_ID: &str = "poltertype";

/// Icon sizes written into the user's `hicolor` theme, each rendered
/// from the geometry rather than filtered down from one master. 16 is
/// absent because `poltertype-icon` refuses a PNG below
/// `MIN_PNG_SIZE`, and nothing on the desktop asks for one.
pub(crate) const HICOLOR_SIZES: &[u32] = &[32, 48, 64, 128, 256];
