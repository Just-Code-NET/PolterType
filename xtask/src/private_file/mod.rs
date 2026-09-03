//! Writing a file only its owner can read.
//!
//! Used for the manifest signing key. The permissions must be in place
//! *before* the secret goes in — writing first and tightening after
//! leaves a window where the key is world-readable — so this is a
//! create, not a chmod.

#[cfg(not(unix))]
mod other;
#[cfg(unix)]
mod unix;

#[cfg(not(unix))]
pub(crate) use other::write;
#[cfg(unix)]
pub(crate) use unix::write;
