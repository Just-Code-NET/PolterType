//! The AppKit side of the macOS tooltip: a borderless, non-activating
//! `NSPanel` whose content view's layer carries the rendered frame.
//! Design spec: `docs/MACOS_POPUP.md`.
//!
//! Everything here runs on the main thread, dispatched from
//! [`super::popup`]; every entry point re-acquires the main-thread
//! marker rather than trusting the caller.
//!
//! **Coordinate spaces.** Anchors arrive in Core Graphics /
//! accessibility coordinates (global, top-left origin, y down) and all
//! placement maths happens there, so [`crate::place`] works unmodified.
//! The single conversion to AppKit's bottom-left space happens when the
//! panel frame is set: `appkit_y = primary_height - cg_y - height`.
//!
//! One `impl` block per concern, one file per `impl` block:
//!
//! | File | Concern |
//! |---|---|
//! | [`state`] | the struct, its fields, construction |
//! | [`types`] | plain data: `Shown` |
//! | [`consts`] | module-wide statics |
//! | [`dispatch`] | main-thread entry points [`super::popup`] calls |
//! | [`show`] | rendering onto the panel |
//! | [`callbacks`] | AppKit clicks/hover and the self-hide timer |
//! | [`geometry`] | scale and placement maths |
//! | [`popup_view`] | the `NSView` subclass forwarding AppKit events |

mod callbacks;
mod consts;
mod dispatch;
mod geometry;
mod popup_view;
mod show;
mod state;
mod types;

pub(super) use dispatch::{hide_on_main, register_events, show_on_main};

#[cfg(test)]
mod tests;
