//! Whether this process may send a system notification yet.
//!
//! Only macOS asks who is sending before the first one, and only macOS
//! punishes a wrong answer with a modal dialog. The others are ready
//! from the start.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(target_os = "macos")]
pub use macos::notification_sender_ready;
#[cfg(not(target_os = "macos"))]
pub use other::notification_sender_ready;
