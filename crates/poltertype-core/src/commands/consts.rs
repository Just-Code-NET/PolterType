//! Reserved ids for the built-in commands.

/// Reserved built-in command ids, so schema validation can warn when a
/// `[[commands]]` entry shadows one.
///
/// The two built-in hotkey actions live in
/// [`crate::settings::HotkeySettings`] and are registered separately by
/// the tray, so a user entry with the same id would still be a text
/// trigger rather than a hotkey replacement — reusing the names is
/// merely confusing, hence the reservation.
pub const BUILTIN_PAUSE_TOGGLE_ID: &str = "pause-toggle";

pub const BUILTIN_SWITCH_LAST_ID: &str = "switch-last";
