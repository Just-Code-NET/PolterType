//! macOS backend for the suggestion tooltip.
//!
//! Same split as the other backends: [`panel`] holds every call into
//! AppKit / Core Graphics, [`popup`] is the public handle and the
//! decisions. The structural difference is threading: AppKit window
//! objects may only be touched on the main thread, and the app's main
//! thread belongs to the tao event loop — so instead of owning a
//! thread this backend hops onto the main dispatch queue, and the
//! state lives in a main-thread `thread_local!` (see
//! `docs/MACOS_POPUP.md`).
//!
//! The focus guarantee is met the same way Windows meets it — by
//! window configuration, not by runtime care: an `NSPanel` with
//! `NSWindowStyleMask::NonactivatingPanel` cannot become key, so a
//! click on a row never takes the keyboard away from the text the
//! user is editing.

mod panel;
mod popup;

pub use popup::MacosPopup;
