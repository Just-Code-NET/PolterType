//! Recent-word history, for triggers made of more than one token.
//!
//! The word buffer resets at every boundary, so `best regards ` needs
//! the engine to remember that `best` came immediately before
//! `regards`. This is that memory.
//!
//! It is the only place the engine keeps more of the user's text than
//! the word being typed, so it is bounded on three axes at once:
//! **length** ([`MAX_HISTORY_WORDS`]), **time** ([`WordHistory::clear`]
//! runs on the idle timeout that clears the word buffer), and
//! **context** (cleared on focus change). It never leaves this process,
//! is never logged — every debug line goes through `redact_word` — and
//! holds only words that ended at a boundary.

use super::{MAX_HISTORY_WORDS, UserCommand};

/// The most recent completed words, oldest first.
#[derive(Debug, Default, Clone)]
pub struct WordHistory {
    words: Vec<String>,
    /// The application these words were typed in, so a change of focus
    /// can drop them. Kept inside the history rather than beside it:
    /// "words from two applications never form a phrase" is an
    /// invariant of this type, not of its callers.
    context: Option<String>,
}

impl WordHistory {
    /// Record a completed word typed in `context`, dropping the oldest
    /// when full.
    ///
    /// A different `context` clears the history first: half a trigger
    /// typed in one window must not combine with a word from another.
    /// An unknown context (`None`, wherever focus tracking does not
    /// answer) is its own single context, so the feature still works
    /// there.
    pub fn push_in(&mut self, context: Option<&str>, word: &str) {
        if self.context.as_deref() != context {
            self.words.clear();
            self.context = context.map(str::to_owned);
        }
        self.push(word);
    }

    /// Record a completed word, dropping the oldest when full.
    pub fn push(&mut self, word: &str) {
        if word.is_empty() {
            return;
        }
        if self.words.len() == MAX_HISTORY_WORDS {
            self.words.remove(0);
        }
        self.words.push(word.to_owned());
    }

    /// Forget everything. Called on the idle timeout, on a focus
    /// change, and after a command fires — see the module docs.
    pub fn clear(&mut self) {
        self.words.clear();
        self.context = None;
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// The last `n` words, oldest first — fewer if that many have not
    /// been typed yet.
    pub fn tail(&self, n: usize) -> &[String] {
        let start = self.words.len().saturating_sub(n);
        &self.words[start..]
    }
}

/// Split a trigger into its tokens. Whitespace-separated, with any
/// run of whitespace treated as one separator so a trigger written
/// with two spaces still matches text typed with one.
pub fn trigger_tokens(trigger: &str) -> Vec<&str> {
    trigger.split_whitespace().collect()
}

/// Does `cmd`'s trigger match the word just completed, given what came
/// before it? A single-token trigger compares against `current_word`; a
/// multi-token one additionally requires its earlier tokens to be the
/// immediately preceding words, in order.
///
/// Case-sensitive, as single-token matching always was — a
/// case-insensitive `best regards` would fire on an ordinary sign-off.
pub fn phrase_matches(cmd: &UserCommand, history: &WordHistory, current_word: &str) -> bool {
    let tokens = trigger_tokens(&cmd.trigger);
    let Some((last, earlier)) = tokens.split_last() else {
        // An all-whitespace trigger matches nothing.
        return false;
    };
    if *last != current_word {
        return false;
    }
    if earlier.is_empty() {
        return true;
    }
    // More leading tokens than we remember: cannot match, and must
    // not match a truncated prefix.
    let preceding = history.tail(earlier.len());
    preceding.len() == earlier.len()
        && preceding
            .iter()
            .zip(earlier)
            .all(|(had, want)| had.as_str() == *want)
}

/// How many on-screen characters a fired command has to erase: the
/// buffered keys plus the boundary, and for a multi-token trigger the
/// earlier words and the separator after each, or half the phrase is
/// left on screen.
///
/// Counts **characters**, because that is what the screen shows and
/// what a backspace removes.
pub fn erase_len(cmd: &UserCommand, current_word_keys: usize) -> usize {
    let tokens = trigger_tokens(&cmd.trigger);
    let earlier: usize = tokens
        .iter()
        .rev()
        .skip(1)
        // +1 for the separator that followed each earlier token.
        .map(|t| t.chars().count() + 1)
        .sum();
    current_word_keys + 1 + earlier
}

#[cfg(test)]
mod tests;
