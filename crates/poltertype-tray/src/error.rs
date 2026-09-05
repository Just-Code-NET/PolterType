//! What can go wrong between a rendered icon and a tray that shows it.

/// A tray operation that did not take.
///
/// Deliberately thin: every variant here is something the caller can
/// only log. A tray that will not update its icon is no reason to stop
/// switching layouts, so the binary warns and carries on.
#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    /// Writing the icon the tray reads back from disk.
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// Whatever the platform's tray library said, in its own words.
    /// Text rather than a type because the backends share no error
    /// type and nothing downstream branches on which one spoke.
    #[error("{0}")]
    Backend(String),

    /// An RGBA buffer whose length does not match its dimensions — a
    /// rasteriser bug, caught here rather than inside a `memcpy`.
    #[error("a {width}×{height} icon needs {want} bytes of RGBA, got {got}")]
    IconSize {
        width: u32,
        height: u32,
        want: usize,
        got: usize,
    },
}
