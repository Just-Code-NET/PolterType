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
    /// Boundary key immediately left of the word being typed, when one
    /// was observed. `/tmp` and `foo tmp` are the same three letters to
    /// every detector; only this says which.
    lead: Option<(u32, bool)>,
    /// `lead` of the completed word in `prev_word`.
    prev_lead: Option<(u32, bool)>,
    /// Set when the current word's tracking is known-unreliable;
    /// cleared at the next boundary. See module docs.
    poisoned: bool,
    /// Is the caret known to sit right after a boundary? False after
    /// clicks / nav / Esc / idle abandons, where the caret may be
    /// mid-word in text we never saw.
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
            lead: None,
            prev_lead: None,
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

    /// The separator the completed word opened after, if the buffer saw
    /// one — valid at the same moment as [`Self::completed`]. `None`
    /// after a caret move, where what precedes the word is unknown.
    pub fn completed_lead(&self) -> Option<(u32, bool)> {
        self.prev_lead
    }

    /// Boundary keys typed since the last completed word, oldest
    /// first. Together with [`Self::completed`] and [`Self::keys`]
    /// this is the full screen model left of the caret — the
    /// suggestion-accept path derives its backspace count from it.
    pub fn boundary_run(&self) -> &[(u32, bool)] {
        &self.boundary_run
    }

    /// Re-point the stash after a suggestion replaced the completed
    /// word on screen, so backspacing across the boundary re-opens the
    /// *new* word. An empty vec (text-injection fallback, scancodes
    /// unknown) just drops it, same as [`Self::forget_completed`].
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

    /// Explicitly taint the in-progress word — the engine's route for
    /// keystrokes racing a correction that it could not attribute.
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    /// Full reset to a clean, trusted state — only when the caller
    /// knows the next word can be trusted (settings reload).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The caret context is gone (shortcut fired, idle gap, focus
    /// change). Drops all stashes; a word in progress leaves an
    /// untracked remainder on screen, so the *next* completion is
    /// tainted.
    ///
    /// Deliberately does not touch `context_clean`: after an *idle*
    /// abandon the caret is almost certainly where it was, so the next
    /// word stays suggestion-eligible. Callers whose trigger really
    /// moves the caret pair this with [`Self::mark_context_unclean`].
    pub fn abandon(&mut self) {
        if !self.keys.is_empty() {
            self.poisoned = true;
        }
        self.keys.clear();
        self.prev_word.clear();
        self.boundary_run.clear();
        self.lead = None;
        self.prev_lead = None;
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
        self.prev_lead = None;
    }

    /// Feed a [`KeyEvent`] with the character it produces under the
    /// **currently active** OS layout (`None` for scancodes with no
    /// mapping at all), plus a hint of whether the same scancode is a
    /// *letter* under any known layout — the hint is what keeps a
    /// Cyrillic word typed under en-US from splitting at every `ж`.
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
                    self.word_clean = self.context_clean;
                    if self.boundary_run.is_empty() && !self.prev_word.is_empty() {
                        // No boundary since the previous completion can
                        // only mean that word was re-opened and fully
                        // backspaced away: it is already `keys`' ancestor
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
                // Any boundary re-syncs tracking: the next word is
                // observed from its first key and starts right after a
                // separator we saw, so the caret cannot be mid-word.
                self.poisoned = false;
                self.context_clean = true;
                // Whatever the last separator before the next word is,
                // that word opened after it.
                let lead = self.lead.replace((ev.scancode, ev.modifiers.shift));
                if self.keys.is_empty() {
                    // A consecutive boundary (double space, ". "):
                    // extend the run guarding the stashed word.
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
                self.prev_lead = lead;
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
                        // Last separator gone: the caret now touches the
                        // previous word, so re-open it with its
                        // start-trust restored.
                        self.keys = std::mem::take(&mut self.prev_word);
                        self.word_clean = self.prev_clean;
                        self.lead = self.prev_lead.take();
                    }
                    return WordBoundary::InProgress;
                }
                // Deleting text the buffer never saw: everything left of
                // the caret is unknown from here on, so the engine must
                // drop caret-dependent state too — hence `Abandoned`.
                self.poisoned = true;
                self.context_clean = false;
                self.lead = None;
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
