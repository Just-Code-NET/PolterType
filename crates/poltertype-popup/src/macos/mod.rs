//! macOS backend for the suggestion tooltip. Design spec:
//! `docs/MACOS_POPUP.md`.
//!
//! [`panel`] holds every call into AppKit / Core Graphics, [`popup`]
//! the public handle. Unlike the other backends it owns no thread:
//! AppKit window objects belong to the main thread, which the tao
//! event loop owns, so commands hop onto the main dispatch queue and
//! state lives in a main-thread `thread_local!`.
//!
//! The focus guarantee comes from window configuration rather than
//! runtime care — an `NSPanel` with `NonactivatingPanel` cannot become
//! key, so a click on a row never takes the keyboard away.

mod panel;
mod popup;

pub(crate) use popup::create_for_platform;
