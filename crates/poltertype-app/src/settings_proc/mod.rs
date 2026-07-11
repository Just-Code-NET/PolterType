//! Settings GUI child-process management.
//!
//! The tray spawns the Settings GUI as a copy of itself (`poltertype
//! --settings`) rather than opening a window in-process — see
//! `settings_ui/mod.rs` for the macOS main-thread rationale. That makes
//! "where is our own binary?" a load-bearing question, which is what
//! `exe.rs` answers; `spawn.rs` does the spawning and the
//! refresh-on-close.

mod consts;
mod enums;
mod exe;
mod spawn;

pub(crate) use spawn::spawn_settings_ui;

#[cfg(test)]
mod tests;
