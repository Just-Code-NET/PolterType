//! Paths and group names the access probe and its messages share.

pub(crate) const EVENT_DEVICE_DIR: &str = "/dev/input";

pub(super) const INPUT_GROUP: &str = "input";

/// The permissions guide, pinned to `main` for the same reason the
/// tray's link is: it has to describe the current setup script, not the
/// release the user happens to be running.
pub(crate) const PERMISSIONS_URL: &str =
    "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md";
