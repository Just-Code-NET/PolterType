//! Outcome of locating the binary this process is running from.

use std::path::PathBuf;

/// Where our own executable is, as far as we can still tell.
///
/// A long-running tray outlives edits to its own binary — a dev rebuild
/// or an in-place upgrade unlinks the file we were started from, and
/// the path we'd hand to `Command::new` stops naming a real file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OwnExe {
    Live(PathBuf),
    /// Spawnable, but a *different* build than this process; the
    /// payload is the real (suffix-stripped) path.
    Replaced(PathBuf),
    /// Uninstall or `cargo clean`: nothing to spawn, payload is only
    /// for the log line.
    Gone(PathBuf),
}

/// Which pane the Settings child process should open on.
///
/// A CLI flag rather than an IPC message: the two processes share
/// nothing at runtime by design, so one argv entry is the whole
/// protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsEntry {
    Normal,
    Setup,
}

impl SettingsEntry {
    pub(super) fn flag(self) -> &'static str {
        match self {
            SettingsEntry::Normal => "--settings",
            SettingsEntry::Setup => "--setup",
        }
    }
}
