//! Why a chord differs from `config.toml`, and how a binding is
//! actually held.

use global_hotkey::hotkey::HotKey;
use poltertype_core::engine::ModChord;

/// Why the chord in force is not the one in `config.toml`.
///
/// A value rather than a sentence: the tray writes it to the log, the
/// Settings window renders it as translated prose, and neither has to
/// know how the other says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Substitution {
    /// The default reaches the focused app as well as us, and
    /// `Ctrl+Backspace` there deletes the very word we are correcting.
    DefaultIsDestructiveHere,
    /// The OS already owns the default chord.
    SystemOwnsDefault,
}

/// One hotkey as it is actually held here: an OS-level grab, or a
/// modifier-only gesture the key stream matches (issue #32).
#[derive(Debug, Clone, Copy)]
pub(crate) enum ActiveBinding {
    Key(HotKey),
    Mods(ModChord),
}

impl ActiveBinding {
    /// Whether an OS hotkey event belongs to this binding. Always false
    /// for a modifier-only chord: nothing registers it, so nothing can
    /// deliver an event for it.
    pub(crate) fn owns_event(self, id: u32) -> bool {
        matches!(self, Self::Key(hk) if hk.id() == id)
    }

    pub(super) fn os_grab(self) -> Option<HotKey> {
        match self {
            Self::Key(hk) if !super::is_lock_key(&hk) => Some(hk),
            _ => None,
        }
    }
}
