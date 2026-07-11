//! Key-direction and switch-action enums.

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDirection {
    Press,
    Release,
}

/// What the engine decided to do with the just-completed word.
#[derive(Debug, Clone, PartialEq)]
pub enum SwitchAction {
    /// Leave everything alone. Carries a human-readable reason for
    /// the tray tooltip / debug log (`"current already a dict word"`,
    /// `"app on disabled list"`, etc.).
    KeepCurrent { reason: String },
    /// Switch the active layout and replay the buffer as the corrected
    /// text.
    SwitchAndReplay {
        target_layout: LayoutId,
        corrected_text: String,
        backspaces: usize,
        reason: String,
    },
}
