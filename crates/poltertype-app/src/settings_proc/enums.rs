//! Outcome of locating the binary this process is running from.

use std::path::PathBuf;

/// Where our own executable is, as far as we can still tell.
///
/// A long-running tray outlives edits to its own binary: a dev rebuild,
/// or a package manager doing an in-place upgrade, unlinks the file we
/// were started from. The process keeps running happily, but the path
/// we'd hand to `Command::new` is no longer the path of a real file —
/// so "spawn a copy of myself" needs to know which of these three
/// worlds it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OwnExe {
    /// The binary we were started from is still there. The normal case.
    Live(PathBuf),
    /// Our binary was replaced on disk while we ran, and a *different*
    /// build now sits at that path. Spawnable — but it is not this
    /// build, and the payload carries the real (suffix-stripped) path.
    Replaced(PathBuf),
    /// Our binary is gone and nothing took its place (uninstall, `cargo
    /// clean`). Nothing to spawn; the payload is the path it lived at,
    /// for the log line.
    Gone(PathBuf),
}

/// Which pane the Settings child process should open on.
///
/// A CLI flag rather than an IPC message because the two processes
/// share nothing at runtime by design — the child reads `config.toml`
/// and exits. One extra argv entry is the entire protocol.
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
