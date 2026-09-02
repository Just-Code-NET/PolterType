//! Step state and the actions a step's button can perform.

/// Where a setup step stands right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// Satisfied. Nothing for the user to do.
    Done,
    /// Not satisfied, and the user can fix it — the step's `action`
    /// says how.
    Todo,
    /// Satisfied on disk but not in *this* session. The specific
    /// Linux trap: `usermod -aG input` updates the group database
    /// immediately and the running session's credentials never, so
    /// everything looks correct and nothing works until the user logs
    /// out. Worth its own state precisely because "Todo" would send
    /// them to re-run a script that has already done its job.
    NeedsRelogin,
    /// The OS has a decision on record and it says no, so its own
    /// prompt will never appear again — the only way through is to
    /// remove the app from the permission list and add it back. The
    /// macOS trap behind it: our bundle is ad-hoc signed, so TCC keys
    /// the grant to the code-directory hash rather than to a team
    /// identifier, and every self-update replaces the bundle and
    /// changes that hash. The switch stays on, the app is denied, and
    /// "Ask macOS now" does nothing (issue #42).
    NeedsReset,
    /// We could not tell. Shown as a neutral note rather than a
    /// warning: claiming a problem we have not proven is how a setup
    /// guide loses the user's trust.
    Unknown,
}

/// The single action a step offers. Rendering and execution live in
/// the app; this crate only says what should happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAction {
    /// Open a URL — a documentation page, or a macOS
    /// `x-apple.systempreferences:` deep link into the exact pane.
    Open(String),
    /// Put a shell command on the clipboard. Used where running it
    /// ourselves would be wrong: `setup-linux.sh` needs `sudo`, and an
    /// app that silently asks for root is an app nobody should trust.
    /// The user reads it, then runs it in their own terminal.
    Copy(String),
    /// Ask the OS to show its own permission prompt (macOS
    /// Accessibility / Input Monitoring). Only ever the *system*
    /// dialog — we never imitate one.
    RequestPermission(Permission),
    /// Create (or adopt) the local code-signing identity that lets
    /// updates keep the TCC grants — see
    /// [`setup_local_signing`](super::setup_local_signing).
    SetupLocalSigning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Accessibility,
    InputMonitoring,
}
