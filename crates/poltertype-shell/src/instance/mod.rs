//! The process-wide single-instance lock.
//!
//! **The id means something different on every platform.**
//! `single-instance` takes a `&str` everywhere but reads it as a named
//! mutex on Windows, an abstract socket name on Linux, and on macOS a
//! **filesystem path** it creates and `flock`s. See [`macos`] for why
//! that path has to be absolute, and [`linux`] for why Linux skips
//! `single-instance` entirely.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod other;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use other as imp;

/// A held single-instance lock.
///
/// What is held is a kernel object, so the lock is released by dropping
/// this *and* by the process ending for any reason, including one that
/// runs no destructors. That is the property being bought: no stale
/// lock after a crash, and no "delete this file to recover" step.
pub use imp::InstanceLock;

/// Try to become the only running instance.
///
/// `Ok(None)` means somebody else already is — the ordinary case, not
/// an error. `Err` means the question could not be answered at all,
/// which is worth reporting rather than guessing either way.
pub use imp::acquire;
