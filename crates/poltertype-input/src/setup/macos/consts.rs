//! System Settings deep links and the `IOHIDCheckAccess` field values.

/// Deep links into the exact System Settings panes. `x-apple.system
/// preferences:` is Apple's documented URL scheme for this; the
/// anchors are the ones the Privacy & Security pane registers.
pub(super) const ACCESSIBILITY_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

pub(super) const INPUT_MONITORING_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

/// System Settings → Notifications (the app list lives inside).
pub(super) const NOTIFICATIONS_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

/// The request type `IOHIDCheckAccess` / `IOHIDRequestAccess` take:
/// `kIOHIDRequestTypeListenEvent` — Input Monitoring.
pub(super) const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;

/// The three `IOHIDCheckAccess` answers. Three different sentences:
/// only "unknown" means the system has not decided, and so only there
/// can a prompt still appear. "Denied" is a record, and a record is
/// what makes the prompt silent.
pub(super) const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
pub(super) const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;
pub(super) const K_IOHID_ACCESS_TYPE_UNKNOWN: u32 = 2;
