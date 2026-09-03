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
mod dispatch;
mod enums;
mod types;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

pub use consts::DEFAULT_LOCAL_SIGNING_IDENTITY;
pub use dispatch::{permission_settings_url, probe_setup, request_permission, setup_local_signing};
pub use enums::{Permission, StepAction, StepState};
pub use types::{SetupReport, SetupStep};

#[cfg(test)]
mod tests;
