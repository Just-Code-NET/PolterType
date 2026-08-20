//! Paths, group names and the links the setup steps point at.

// ─── Linux ────────────────────────────────────────────────────────────

/// Owned by the input backend: the pane has to probe exactly the place
/// the listener opens.
#[cfg(target_os = "linux")]
pub(super) use crate::linux::access::{EVENT_DEVICE_DIR, PERMISSIONS_URL};

#[cfg(target_os = "linux")]
pub(super) const UINPUT_DEVICE: &str = "/dev/uinput";

/// What we put on the clipboard rather than run — resolved against the
/// running binary, so an AppImage names the copy it carries.
#[cfg(target_os = "linux")]
pub(super) use crate::linux::access::setup_script_command;

// ─── macOS ────────────────────────────────────────────────────────────

/// Deep links into the exact System Settings panes. `x-apple.system
/// preferences:` is Apple's documented URL scheme for this; the
/// anchors are the ones the Privacy & Security pane registers.
#[cfg(target_os = "macos")]
pub(super) const ACCESSIBILITY_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[cfg(target_os = "macos")]
pub(super) const INPUT_MONITORING_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
