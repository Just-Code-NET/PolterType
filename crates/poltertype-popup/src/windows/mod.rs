//! Windows backend for the suggestion tooltip.
//!
//! Split the same way the X11 backend is: [`window`] holds every call
//! into Win32, [`popup`] holds the thread and the decisions. That keeps
//! the loop readable as ordinary Rust and puts all the `unsafe` behind
//! one door.
//!
//! The focus guarantee this crate demands of every backend is met by
//! the extended window styles rather than by any care taken at runtime:
//! a `WS_EX_NOACTIVATE` window cannot be activated by clicking it, so
//! there is no path by which the tooltip takes the keyboard away from
//! whatever the user is typing into. See [`window::PopupWindow::create`].

mod consts;
mod popup;
mod window;

#[cfg(test)]
mod tests;

pub use popup::WindowsPopup;
