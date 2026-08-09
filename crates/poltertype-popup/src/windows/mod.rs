//! Windows backend for the suggestion tooltip.
//!
//! Split the way the X11 backend is: [`window`] holds every call into
//! Win32, [`popup`] the thread and the decisions, which keeps the loop
//! readable as ordinary Rust and puts all the `unsafe` behind one door.
//!
//! The crate's focus guarantee is met by the extended window styles
//! rather than by care taken at runtime — a `WS_EX_NOACTIVATE` window
//! cannot be activated by clicking it. See
//! [`window::PopupWindow::create`].

mod consts;
mod popup;
mod window;

#[cfg(test)]
mod tests;

pub use popup::WindowsPopup;
