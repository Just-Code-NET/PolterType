//! `WordBuffer` — the scancode-level word accumulator.

use super::*;
use poltertype_input::{KeyDirection, KeyEvent};
use poltertype_types::WordKey;

#[derive(Debug, Default)]
pub struct WordBuffer {
    keys: Vec<WordKey>,
    /// Keys of the most recently completed word — still on screen
    /// immediately left of `boundary_run`. Cleared whenever the
    /// screen can no longer be assumed to contain them.
    prev_word: Vec<WordKey>,
    /// Boundary keys (scancode, shift) typed since the last completed
    /// word, oldest first. Screen layout is
    /// `…<prev_word><boundary_run><keys>[caret]`.
    boundary_run: Vec<(u32, bool)>,
    /// See module docs. Set when the current word's tracking is
    /// known-unreliable; cleared at the next boundary.
    poisoned: bool,
}

impl WordBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keys(&self) -> &[WordKey] {
        &self.keys
    }

    /// Keys of the most recently completed word (valid right after
    /// [`feed`](Self::feed) returned [`WordBoundary::WordCompleted`]).
    pub fn completed(&self) -> &[WordKey] {
        &self.prev_word
    }

    pub fn poisoned(&self) -> bool {
        self.poisoned
    }

    /// Explicitly taint the in-progress word (used by the engine when
    /// it detects keystrokes racing a correction that it could not
    /// attribute reliably).
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    /// Full reset to a clean, trusted state. Only appropriate when
    /// the caller knows tracking should restart from scratch and the
    /// next word can be trusted (settings reload).
    pub fn reset(&mut self) {
        self.keys.clear();
        self.prev_word.clear();
        self.boundary_run.clear();
        self.poisoned = false;
    }

    /// The caret context is gone (shortcut fired, idle gap, focus
    /// change). Drops all stashes; if a word was in progress its
    /// remainder is untracked on screen, so the *next* completion is
    /// tainted.
    pub fn abandon(&mut self) {
        if !self.keys.is_empty() {
            self.poisoned = true;
        }
        self.keys.clear();
        self.prev_word.clear();
        self.boundary_run.clear();
    }

    /// The just-completed word no longer exists on screen (a smart
    /// command erased and replaced it). Forget it so backspace never
    /// tries to re-open it.
    pub fn forget_completed(&mut self) {
        self.prev_word.clear();
        self.boundary_run.clear();
    }

    /// Feed a [`KeyEvent`] together with the character it produces
    /// under the **currently active** OS layout, plus a hint of
    /// whether the same scancode is a *letter* under any of the
    /// engine's known layouts. The hint catches the
    /// "user is typing a Cyrillic word while the en-US layout is
    /// active" case: scancode `0x27` is `;` in en-US (a boundary)
    /// but `ж` in uk-UA (a word character). Without
    /// `letter_in_any_layout` the buffer would split mid-word at
    /// every such position.
    ///
    /// Pass `produced = None` if the scancode has no mapping at all
    /// (control / function keys).
    pub fn feed(
        &mut self,
        ev: KeyEvent,
        produced: Option<char>,
        letter_in_any_layout: bool,
    ) -> WordBoundary {
        if ev.direction != KeyDirection::Press {
            return WordBoundary::InProgress;
        }

        match classify(ev.scancode, produced, letter_in_any_layout) {
            KeyKind::Word => {
                if self.keys.is_empty()
                    && self.boundary_run.is_empty()
                    && !self.prev_word.is_empty()
                {
                    // A word key with no boundary since the previous
                    // completion can only mean the previous word was
                    // re-opened and fully backspaced away, then typing
                    // resumed — prev_word is already `keys`' ancestor
                    // and must not be re-openable behind it.
                    self.prev_word.clear();
                }
                self.keys.push(WordKey {
                    scancode: ev.scancode,
                    shift: ev.modifiers.shift,
                    timestamp_ms: ev.timestamp_ms,
                });
                WordBoundary::InProgress
            }
            KeyKind::Boundary => {
                let tainted = self.poisoned;
                // Any boundary re-syncs tracking: whatever went wrong
                // before it, the next word is observed from its first
                // key.
                self.poisoned = false;
                if self.keys.is_empty() {
                    // No word completed — this is a consecutive
                    // boundary (double space, ". "). Extend the run
                    // guarding the stashed word, up to a sane limit.
                    if !self.prev_word.is_empty() {
                        if tainted {
                            self.forget_completed();
                        } else if self.boundary_run.len() < MAX_BOUNDARY_RUN {
                            self.boundary_run.push((ev.scancode, ev.modifiers.shift));
                        } else {
                            self.forget_completed();
                        }
                    }
                    return WordBoundary::InProgress;
                }
                self.prev_word = std::mem::take(&mut self.keys);
                self.boundary_run.clear();
                self.boundary_run.push((ev.scancode, ev.modifiers.shift));
                if tainted {
                    // Never re-open (or auto-correct) a word we only
                    // partially observed.
                    self.forget_completed();
                }
                WordBoundary::WordCompleted {
                    boundary_scancode: ev.scancode,
                    boundary_shift: ev.modifiers.shift,
                    tainted,
                }
            }
            KeyKind::Backspace => {
                if self.keys.pop().is_some() {
                    return WordBoundary::InProgress;
                }
                if self.boundary_run.pop().is_some() {
                    if self.boundary_run.is_empty() {
                        // Deleted the last separator — the caret now
                        // touches the previous word. Re-open it.
                        self.keys = std::mem::take(&mut self.prev_word);
                    }
                    return WordBoundary::InProgress;
                }
                // Deleting text the buffer never saw — everything to
                // the left of the caret is unknown from here on. The
                // engine must also drop caret-position-dependent
                // state (the switch-last stash), hence `Abandoned`.
                self.poisoned = true;
                WordBoundary::Abandoned
            }
            KeyKind::Discard => WordBoundary::InProgress,
            KeyKind::EndAndDiscard => {
                self.abandon();
                WordBoundary::Abandoned
            }
        }
    }
}
