//! Per-OS foreground-app tracking.
//!
//! The engine consults this to decide whether auto-switching is
//! appropriate for the focused application — the dev-friendly path
//! that keeps the corrector silent in IDEs / terminals while you're
//! typing code (see `docs/DECISIONS.md`).
//!
//! The trait is intentionally minimal: just "what's the executable
//! name of the focused window?". That's enough to match against the
//! `[exceptions].disabled_apps` list. Window class / title matching
//! land in v0.1.x if needed.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod windows_impl;

mod factory;
mod noop;
mod traits;

pub use factory::*;
pub use noop::*;
pub use traits::*;
