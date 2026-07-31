//! The setup walkthrough's model: what the user still has to grant.
//!
//! When the keyboard hooks fail to start, the tray has always shown an
//! alert and a link to `docs/PERMISSIONS.md`. That stops the app
//! failing *silently*, and then leaves the user reading a markdown
//! file to fix their own machine. This module is the data the Settings
//! window's **Setup** pane renders instead: the same advice, but
//! specific to this OS, this session, and re-checkable after the user
//! has changed something.
//!
//! It lives here because probing input permissions is platform code,
//! and platform code lives in this crate (see the workspace
//! `CLAUDE.md`). The app renders [`SetupReport`]; it does not know
//! what a udev rule is.
//!
//! **Nothing here changes the system.** The most a step does is open a
//! documentation page or a System Settings pane, put a command on the
//! clipboard, or ask macOS to show its own permission dialog. The
//! Linux script needs `sudo`, and an app that quietly acquires root
//! has spent trust it will not get back — so the user runs it, in
//! their terminal, having read it.

mod consts;
mod enums;
mod types;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

pub use enums::{Permission, StepAction, StepState};
pub use types::{SetupReport, SetupStep};

/// Probe the current machine. Cheap enough to call on every *Check
/// again* click — a handful of `stat`s and one framework call — and
/// deliberately not cached, since the entire point is to notice that
/// the user just flipped a switch.
pub fn probe_setup() -> SetupReport {
    #[cfg(target_os = "linux")]
    {
        linux::probe()
    }
    #[cfg(target_os = "macos")]
    {
        macos::probe()
    }
    #[cfg(windows)]
    {
        SetupReport {
            backend: Some("windows-ll-hook".to_owned()),
            steps: vec![SetupStep {
                title: "Nothing to set up on Windows".to_owned(),
                detail: "The low-level keyboard hook PolterType uses needs no permission and \
                         no elevation — it works from a normal user account the moment the \
                         app starts."
                    .to_owned(),
                state: StepState::Done,
                action: None,
            }],
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        SetupReport {
            backend: None,
            steps: Vec::new(),
        }
    }
}

/// Trigger the OS's own permission dialog (macOS only).
///
/// Returns whether the permission is granted after the call. On
/// Accessibility that answer is usually `false` even when all is well:
/// the dialog is asynchronous, so the honest reading is "the user has
/// been asked", and the pane re-probes rather than believing this.
/// Everywhere else there is no such dialog and this is a no-op.
pub fn request_permission(permission: Permission) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::request(permission)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        false
    }
}

/// Where to send a user whose system dialog will never appear again —
/// macOS shows each prompt once, and after that the only route is the
/// Settings pane itself.
pub fn permission_settings_url(permission: Permission) -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::settings_pane_url(permission))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        None
    }
}

#[cfg(test)]
mod tests;
