//! Windows global keyboard listener, emitter and key gate.
//!
//! The gate's swallow decision lives one level up in `crate::hold`: no
//! OS dependency, shared with the macOS gate, so it compiles under
//! `cfg(test)` on a project that owns no Windows machine. Everything
//! touching Win32 is `#[cfg(windows)]`, compiled only by CI's
//! `windows-latest` job.

#[cfg(windows)]
mod consts;
#[cfg(windows)]
mod emitter;
#[cfg(windows)]
pub(crate) mod factory;
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
