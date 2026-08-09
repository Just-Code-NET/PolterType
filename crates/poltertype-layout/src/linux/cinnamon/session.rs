//! Are we in a Cinnamon session?
//!
//! Cinnamon's `.desktop` files carry no `DesktopNames=`, so which
//! variable ends up set is the display manager's business. All three
//! candidates are asked rather than one picked: a session that answers
//! to any of them is Cinnamon, and being wrong the other way hands the
//! session back to a backend that cannot switch it.

use super::*;

/// Every variable a session manager might announce the desktop in.
pub(crate) const DESKTOP_VARS: [&str; 3] = [
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DESKTOP_SESSION",
];

/// Spellings of the session name, across the variables above and the
/// distros that write them.
const CINNAMON_NAMES: [&str; 4] = ["cinnamon", "x-cinnamon", "cinnamon2d", "x-cinnamon2d"];

/// Does this variable's value name Cinnamon?
///
/// The value is a colon-separated list, and some display managers write
/// each entry as a full path to the session file. Entries are matched
/// whole rather than by substring, which would also claim a
/// `Cinnamon-something` fork whose input stack we have never seen.
pub(crate) fn names_cinnamon(value: &str) -> bool {
    value.split(':').any(|entry| {
        let entry = entry.trim().rsplit('/').next().unwrap_or_default();
        CINNAMON_NAMES
            .iter()
            .any(|known| entry.eq_ignore_ascii_case(known))
    })
}

pub(crate) fn session_is_cinnamon() -> bool {
    DESKTOP_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok_and(|value| names_cinnamon(&value)))
}
