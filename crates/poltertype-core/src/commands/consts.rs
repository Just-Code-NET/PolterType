//! Reserved ids for the built-in commands, and the limits smart
//! commands run under.

use std::time::Duration;

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

/// How many completed words to remember. Four covers `with best
/// regards` and the like; a longer window would buy vanishingly rare
/// triggers at the cost of holding more of the user's text in memory.
pub const MAX_HISTORY_WORDS: usize = 4;

/// Longest a triggered command may run before it is abandoned.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(5);

/// Most stdout bytes kept when `insert_output` is set. A command that
/// prints a megabyte should not have a megabyte typed into the user's
/// editor one keystroke at a time.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024;
