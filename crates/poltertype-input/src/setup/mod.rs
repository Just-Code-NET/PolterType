//! The setup walkthrough's model: what the user still has to grant.
//!
//! The data the Settings window's **Setup** pane renders: the advice of
//! `docs/PERMISSIONS.md`, specific to this OS and session, re-checkable
//! after the user changes something.
//!
//! It lives in this crate because probing input permissions is platform
//! code. The app renders [`SetupReport`]; it does not know what a udev
//! rule is.
//!
//! **Nothing here changes the system.** A step opens a documentation
//! page or a settings pane, copies a command, or asks macOS to show its
//! own dialog. The Linux script needs `sudo`, and an app that quietly
//! acquires root has spent trust it will not get back.

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
/// `local_signing_identity` is `[updates].local_signing_identity` — the
/// macOS pane adds a step about keeping permissions across updates and
/// needs to know whether the machinery is already configured. The
/// other platforms ignore it.
pub fn probe_setup(local_signing_identity: &str) -> SetupReport {
    let _ = local_signing_identity;
    #[cfg(target_os = "linux")]
    {
        linux::probe()
    }
    #[cfg(target_os = "macos")]
    {
        macos::probe(local_signing_identity)
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


/// The identity name [`setup_local_signing`] creates when the config
/// does not name one.
pub const DEFAULT_LOCAL_SIGNING_IDENTITY: &str = "PolterType Local Signing";

/// Create (or adopt) the local code-signing identity the updater
/// re-signs swapped bundles with, so the TCC grants survive updates.
///
/// macOS only. Idempotent: an identity of that name already in the
/// keychain is adopted rather than duplicated. The caller writes the
/// name into `[updates].local_signing_identity` on `Ok`.
pub fn setup_local_signing(name: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::setup_local_signing(name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        Err("local update signing is a macOS mechanism".to_owned())
    }
}


#[cfg(test)]
mod tests;
