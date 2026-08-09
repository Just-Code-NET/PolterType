//! Windows global keyboard listener, emitter and key gate.
//!
//! A directory because the gate's swallow decision lives one level up,
//! in `crate::hold`: it carries no OS dependency, is shared with the
//! macOS gate, and so compiles under `cfg(test)` on any host. That
//! matters more here than anywhere else — the property being tested is
//! "the user's keyboard always comes back", and this project has no
//! Windows machine to discover otherwise on.
//!
//! Everything touching Win32 is `#[cfg(windows)]`, compiled only by
//! CI's `windows-latest` job.

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
