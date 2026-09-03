//! Spawning a plug-in's process, and asking it to stop.
//!
//! Only two operations in "supervise a plug-in" are per-platform: how a
//! child is created, which only Windows has an opinion about
//! ([`configure_child`]), and how it is asked to leave, which only Unix
//! has a mechanism for ([`request_stop`]).

#[cfg(not(unix))]
mod not_unix;
#[cfg(not(windows))]
mod not_windows;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(not(unix))]
pub use not_unix::request_stop;
#[cfg(not(windows))]
pub use not_windows::configure_child;
#[cfg(unix)]
pub use unix::request_stop;
#[cfg(windows)]
pub use windows::configure_child;
