//! Windows global keyboard listener, emitter and key gate.
//!
//! ## Why this is a directory
//!
//! The key gate's swallow decision lives one level up, in
//! `crate::hold` — it carries no OS dependency and is shared with the
//! macOS gate, so it compiles under `cfg(test)` on any host and its
//! tests run in CI on Linux and macOS too. That matters more here
//! than anywhere else in the crate: the property being tested is "the
//! user's keyboard always comes back", and this project has no Windows
//! machine to discover otherwise on.
//!
//! Everything that touches Win32 is `#[cfg(windows)]` and is compiled
//! only by CI's `windows-latest` job.

#[cfg(windows)]
mod consts;
#[cfg(windows)]
mod emitter;
#[cfg(windows)]
mod gate;
#[cfg(windows)]
mod listener;

#[cfg(windows)]
pub use emitter::WindowsEmitter;
#[cfg(windows)]
pub use gate::WindowsGate;
#[cfg(windows)]
pub use listener::WindowsListener;
