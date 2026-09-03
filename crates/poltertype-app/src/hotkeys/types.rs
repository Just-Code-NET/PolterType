//! The chord actually in force, and what is currently registered.

use super::enums::{ActiveBinding, Substitution};

/// The chord a hotkey answers to on this machine.
///
/// Both substitutions apply **only while the user is still on the
/// default** — an explicit binding is always honoured — and neither is
/// written back to `config.toml`, so one config file keeps meaning the
/// same thing on every machine. That is also why this is resolved in
/// two places at once and must stay one function: the tray decides
/// what to listen for, the Settings window decides what to show, and
/// they disagreed for a whole release (issue #31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EffectiveHotkey<'a> {
    pub(crate) chord: &'a str,
    pub(crate) substitution: Option<Substitution>,
}

/// The two chords in force right now, and — through their ids — what
/// the event loop dispatches an OS hotkey event on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActiveHotkeys {
    pub(crate) pause: ActiveBinding,
    pub(crate) switch_last: ActiveBinding,
}
