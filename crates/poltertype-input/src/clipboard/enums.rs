//! Why the clipboard is unreachable, worded for the Setup pane.

/// Why the clipboard is unavailable here, in words a Setup pane can
/// show without the reader knowing what a Wayland protocol is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardGap {
    /// The compositor offers no data-control protocol, so the only way
    /// to read the clipboard would be to take focus.
    NoWindowlessAccess,
    /// The platform has a clipboard but this build could not open it.
    Unavailable(String),
    /// The clipboard is fine; this build cannot press the copy chord
    /// that would fill it. macOS, until its emitter grows `send_chord`.
    NoCopyChord,
}

impl std::fmt::Display for ClipboardGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowlessAccess => write!(
                f,
                "this desktop does not let a background app read the clipboard \
                 without taking keyboard focus, which PolterType will not do"
            ),
            Self::Unavailable(why) => write!(f, "the clipboard could not be opened: {why}"),
            Self::NoCopyChord => write!(
                f,
                "PolterType cannot yet send the copy shortcut on this platform, \
                 so it has no way to read what you have selected"
            ),
        }
    }
}
