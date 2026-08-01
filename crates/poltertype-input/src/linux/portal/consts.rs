//! Portal names, numbers and our own budgets.

/// The portal service, and the object every call goes to.
pub const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
pub const REMOTE_DESKTOP_IFACE: &str = "org.freedesktop.portal.RemoteDesktop";
pub const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";
pub const SESSION_IFACE: &str = "org.freedesktop.portal.Session";

/// `DeviceType` bitmask — we only ever ask for the keyboard. Asking
/// for the pointer as well would widen the consent dialog to
/// something PolterType has no use for.
pub const DEVICE_KEYBOARD: u32 = 1;

/// `KeyState`.
pub const KEY_RELEASED: u32 = 0;
pub const KEY_PRESSED: u32 = 1;

/// `PersistMode::Persistent` — ask the portal to remember this grant
/// so later runs do not re-prompt. The compositor decides whether to
/// honour it; a restore token only comes back if it does.
pub const PERSIST_PERSISTENT: u32 = 2;

/// How long to wait for a portal `Response` signal.
///
/// Generous on purpose: `Start` puts a dialog in front of a human,
/// and a human takes longer than any protocol. The other calls answer
/// immediately, so this is really the deadline for "the user walked
/// away" — at which point failing is right, and the emitter falls
/// back to whatever the caller had before.
pub const RESPONSE_TIMEOUT_SECS: u64 = 120;

/// Portal `Response` codes.
pub const RESPONSE_SUCCESS: u32 = 0;
pub const RESPONSE_CANCELLED: u32 = 1;

/// File under `<data_local_dir>/poltertype/` holding the restore
/// token, so a granted session is not re-prompted every launch.
///
/// Not in `config.toml`: it is an opaque credential the portal issued
/// to this install, not a setting anyone should edit, and it has no
/// business in a file people paste into bug reports.
pub const RESTORE_TOKEN_FILE: &str = "portal-restore-token";
