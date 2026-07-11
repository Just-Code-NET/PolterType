//! Reserved ids for the built-in commands.

/// Reserved built-in command ids — defined here so user-side schema
/// validation can warn when a `[[commands]]` entry tries to shadow
/// one. The engine's two built-in hotkey actions
/// (`pause-toggle` and `switch-last`) live in [`crate::settings::HotkeySettings`]
/// and are registered separately by the tray; a user-side
/// `[[commands]]` entry with the same id would still be a smart
/// command (text trigger), not a hotkey replacement — but reusing
/// the names is confusing, hence the reservation.
pub const BUILTIN_PAUSE_TOGGLE_ID: &str = "pause-toggle";

pub const BUILTIN_SWITCH_LAST_ID: &str = "switch-last";
