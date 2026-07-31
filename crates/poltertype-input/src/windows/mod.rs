//! Windows global keyboard listener, emitter and key gate.
//!
//! ## Why this is a directory
//!
//! `hold` holds the key gate's decision — whether a given keystroke is
//! kept from the focused application — and carries no `windows-rs`
//! dependency, so it compiles under `cfg(test)` on any host and its
//! tests run in CI on Linux and macOS too. That matters more here than
//! anywhere else in the crate: the property being tested is "the user's
//! keyboard always comes back", and this project has no Windows machine
//! to discover otherwise on.
//!
//! Everything that touches Win32 is `#[cfg(windows)]` and is compiled
//! only by CI's `windows-latest` job.

// Compiled under `cfg(test)` everywhere; see above.
pub(crate) mod hold;

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
