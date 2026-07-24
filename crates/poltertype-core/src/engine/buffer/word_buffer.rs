//! `WordBuffer` — the scancode-level word accumulator.

use super::*;
use poltertype_input::{KeyDirection, KeyEvent};
use poltertype_types::WordKey;

#[derive(Debug)]
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
    /// Is the caret known to sit right after a boundary (start of
    /// input, or an observed separator)? False after clicks / nav /
    /// Esc / idle abandons, where the caret may be mid-word in text
    /// we never saw. Captured into `word_clean` when a word's first
    /// key arrives.
    context_clean: bool,
    /// `context_clean` at the moment the in-progress word started.
    word_clean: bool,
    /// `word_clean` of the completed word in `prev_word` (restored on
    /// backspace re-open).
    prev_clean: bool,
}

impl Default for WordBuffer {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            prev_word: Vec::new(),
            boundary_run: Vec::new(),
            poisoned: false,
            // Fresh tracking starts trusted: nothing is left of the
            // caret that we could be splitting.
            context_clean: true,
            word_clean: true,
            prev_clean: true,
        }
    }
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

    /// Boundary keys typed since the last completed word, oldest
    /// first. Together with [`Self::completed`] and [`Self::keys`]
    /// this is the full screen model left of the caret — the
    /// suggestion-accept path derives its backspace count from it.
    pub fn boundary_run(&self) -> &[(u32, bool)] {
        &self.boundary_run
    }

    /// The completed word was replaced on screen with different
    /// scancodes (a suggestion was applied). Keep the stash coherent
    /// so backspacing across the boundary re-opens the *new* word.
    /// Pass an empty vec when the replacement's scancodes are unknown
    /// (text-injection fallback) — the word simply stops being
    /// re-openable, same as [`Self::forget_completed`].
    pub fn replace_completed(&mut self, keys: Vec<WordKey>) {
        if keys.is_empty() {
            self.forget_completed();
        } else {
            self.prev_word = keys;
        }
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
        *self = Self::default();
    }

    /// The caret context is gone (shortcut fired, idle gap, focus
    /// change). Drops all stashes; if a word was in progress its
    /// remainder is untracked on screen, so the *next* completion is
    /// tainted.
    ///
    /// Deliberately does NOT touch `context_clean`: an *idle* abandon
    /// is buffer hygiene — the user paused to think and the caret is
    /// almost certainly still where it was, so the next word must
    /// stay suggestion-eligible. Callers whose trigger actually moves
    /// the caret (clicks, nav, Esc, shortcuts) pair this with
    /// [`Self::mark_context_unclean`] — the click/nav classify path
    /// does it internally.
    pub fn abandon(&mut self) {
        if !self.keys.is_empty() {
            self.poisoned = true;
        }
        self.keys.clear();
        self.prev_word.clear();
        self.boundary_run.clear();
    }

    /// The caret may now be anywhere — including mid-word in text the
    /// buffer never saw. The next word starts unclean (no suggestion
    /// tooltip) until a boundary is observed again.
    pub fn mark_context_unclean(&mut self) {
        self.context_clean = false;
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
                if self.keys.is_empty() {
                    // First key of a word — freeze the caret-context
                    // trust into the word itself.
                    self.word_clean = self.context_clean;
                    if self.boundary_run.is_empty() && !self.prev_word.is_empty() {
                        // A word key with no boundary since the previous
                        // completion can only mean the previous word was
                        // re-opened and fully backspaced away, then typing
                        // resumed — prev_word is already `keys`' ancestor
                        // and must not be re-openable behind it.
                        self.prev_word.clear();
                    }
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
                // key — and starts right after a separator we saw, so
                // the caret cannot be mid-word any more.
                self.poisoned = false;
                self.context_clean = true;
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
                let started_clean = self.word_clean;
                self.prev_word = std::mem::take(&mut self.keys);
                self.prev_clean = started_clean;
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
                    started_clean,
                }
            }
            KeyKind::Backspace => {
                if self.keys.pop().is_some() {
                    return WordBoundary::InProgress;
                }
                if self.boundary_run.pop().is_some() {
                    if self.boundary_run.is_empty() {
                        // Deleted the last separator — the caret now
                        // touches the previous word. Re-open it,
                        // restoring its start-trust too.
                        self.keys = std::mem::take(&mut self.prev_word);
                        self.word_clean = self.prev_clean;
                    }
                    return WordBoundary::InProgress;
                }
                // Deleting text the buffer never saw — everything to
                // the left of the caret is unknown from here on. The
                // engine must also drop caret-position-dependent
                // state (the switch-last stash), hence `Abandoned`.
                self.poisoned = true;
                self.context_clean = false;
                WordBoundary::Abandoned
            }
            KeyKind::Discard => WordBoundary::InProgress,
            KeyKind::EndAndDiscard => {
                // Click / nav / Esc — the caret genuinely moved, and
                // may now sit mid-word in text we never observed.
                self.abandon();
                self.mark_context_unclean();
                WordBoundary::Abandoned
            }
        }
    }
}
