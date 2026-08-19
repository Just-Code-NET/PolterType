//! Word-boundary and key-classification enums.

/// What the buffer's [`feed`](WordBuffer::feed) just observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordBoundary {
    /// Key absorbed; word still in progress (or no-op for releases /
    /// non-text keys).
    InProgress,
    /// User finished a word. The boundary key has **already been
    /// delivered** to the focused app — the engine must include it in
    /// its backspace count and re-emit a copy after the correction.
    WordCompleted {
        boundary_scancode: u32,
        boundary_shift: bool,
        /// The buffer lost track of this word at some point (caret
        /// moved, idle gap, edit past the word start). The engine
        /// must not auto-correct it and must drop any stashed
        /// last-word state — the screen no longer matches what we
        /// recorded.
        tainted: bool,
        /// The word's first key arrived right after an *observed*
        /// boundary, rather than after a click, navigation, Esc or idle
        /// abandon. An unclean start means the typed keys may be a
        /// fragment of a longer on-screen word, so a suggestion computed
        /// on them would splice a replacement into its middle. Auto
        /// layout-correction is deliberately *not* gated on this.
        started_clean: bool,
    },
    /// Caret moved to an unknown place. Word tracking restarted; the
    /// engine must invalidate anything assuming the old caret position
    /// (e.g. the manual switch-last stash).
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyKind {
    /// Letter / digit / apostrophe / hyphen — accumulate.
    Word,
    /// Whitespace / sentence punctuation / brackets — end the word.
    Boundary,
    Backspace,
    /// Modifier alone, NumLock, function key — ignore.
    Discard,
    /// Esc, arrows, Home/End, mouse click — caret went somewhere we
    /// can't see; abandon tracking.
    EndAndDiscard,
}
