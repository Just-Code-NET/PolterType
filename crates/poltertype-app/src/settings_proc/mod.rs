//! Settings GUI child-process management.
//!
//! The tray spawns the Settings GUI as a copy of itself rather than
//! opening a window in-process (see `docs/ARCHITECTURE.md`), which
//! makes "where is our own binary?" a load-bearing question. `exe.rs`
//! answers it; `spawn.rs` does the spawning and the refresh-on-close.

mod consts;
mod enums;
mod exe;
mod spawn;

pub(crate) use spawn::{kill_settings_ui, spawn_settings_ui, spawn_setup_ui};

#[cfg(test)]
mod tests;
