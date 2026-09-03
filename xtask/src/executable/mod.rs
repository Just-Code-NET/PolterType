//! Restoring the executable bit on the hook scripts.
//!
//! A capability with one real implementation: Git for Windows runs
//! hooks through its bundled `sh.exe` whatever the file mode says, so
//! the Windows half has nothing to do and says so.

#[cfg(not(unix))]
mod other;
#[cfg(unix)]
mod unix;

#[cfg(not(unix))]
pub(crate) use other::mark_all;
#[cfg(unix)]
pub(crate) use unix::mark_all;
