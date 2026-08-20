//! Paths, group names and the links the setup steps point at.

// ─── Linux ────────────────────────────────────────────────────────────

/// Owned by the input backend: the pane has to probe exactly the place
/// the listener opens.
#[cfg(target_os = "linux")]
pub(super) use crate::linux::access::{EVENT_DEVICE_DIR, PERMISSIONS_URL};

#[cfg(target_os = "linux")]
pub(super) const UINPUT_DEVICE: &str = "/dev/uinput";

/// What we put on the clipboard rather than run.
///
/// The script needs `sudo`, and an app that quietly asks for root has
/// spent trust it will not get back. Handing over a command the user
/// can read, in a terminal they opened, keeps the decision theirs. The
/// `curl`-free form assumes a checkout or an unpacked AppImage; the
/// guide covers the rest.
#[cfg(target_os = "linux")]
pub(super) const SETUP_SCRIPT_COMMAND: &str = "bash scripts/setup-linux.sh";

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
