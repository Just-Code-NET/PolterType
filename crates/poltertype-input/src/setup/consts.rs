//! Paths, group names and the links the setup steps point at.

/// The permissions guide, pinned to `main` for the same reason the
/// tray's link is: it has to describe the current setup script, not
/// the release the user happens to be running.
pub(super) const PERMISSIONS_URL: &str =
    "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md";

// ─── Linux ────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub(super) const EVENT_DEVICE_DIR: &str = "/dev/input";

#[cfg(target_os = "linux")]
pub(super) const UINPUT_DEVICE: &str = "/dev/uinput";

#[cfg(target_os = "linux")]
pub(super) const INPUT_GROUP: &str = "input";

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
