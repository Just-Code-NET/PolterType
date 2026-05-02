//! Word buffer: accumulate keystrokes until the user finishes a word.
//!
//! "Finishes" = produces a key that we treat as a word boundary
//! (Space, Enter, Tab, Backspace, ., , ; : ! ?). Backspace also
//! deletes the most recent buffered key.

use kb_input::{KeyDirection, KeyEvent};
use kb_types::WordKey;

/// What the buffer's [`feed`](WordBuffer::feed) just observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordBoundary {
    /// Key absorbed; word still in progress (or no-op for releases /
    /// non-text keys).
    InProgress,
    /// User finished a word (Space, Enter, …). The boundary key has
    /// **already been delivered** to the focused app — the engine
    /// must include it in its backspace count and re-emit a copy
    /// after the correction.
    WordCompleted {
        boundary_scancode: u32,
        boundary_shift: bool,
    },
}

#[derive(Debug, Default)]
pub struct WordBuffer {
    keys: Vec<WordKey>,
}

impl WordBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keys(&self) -> &[WordKey] {
        &self.keys
    }

    pub fn clear(&mut self) {
        self.keys.clear();
    }

    pub fn start_new_word(&mut self) {
        self.keys.clear();
    }

    /// Snapshot the current buffer and drain it. Used by the engine
    /// when deciding on the just-completed word.
    pub fn take_word(&mut self) -> Vec<WordKey> {
        std::mem::take(&mut self.keys)
    }

    /// Feed a [`KeyEvent`]; returns whether it ended a word.
    pub fn feed(&mut self, ev: KeyEvent) -> WordBoundary {
        if ev.direction != KeyDirection::Press {
            return WordBoundary::InProgress;
        }

        match classify(ev.scancode) {
            KeyKind::Word => {
                self.keys.push(WordKey {
                    scancode: ev.scancode,
                    shift: ev.modifiers.shift,
                    timestamp_ms: ev.timestamp_ms,
                });
                WordBoundary::InProgress
            }
            KeyKind::Boundary => WordBoundary::WordCompleted {
                boundary_scancode: ev.scancode,
                boundary_shift: ev.modifiers.shift,
            },
            KeyKind::Backspace => {
                self.keys.pop();
                WordBoundary::InProgress
            }
            KeyKind::Discard => {
                // Arrow keys, modifiers alone, etc — ignore but
                // don't end the word.
                WordBoundary::InProgress
            }
            KeyKind::EndAndDiscard => {
                // Mouse, Esc, navigation — end the word but engine
                // generally won't act on it; we drop the buffer.
                self.keys.clear();
                WordBoundary::InProgress
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    /// Letter / digit / printable punctuation we treat as part of words.
    Word,
    /// Word-boundary key (Space, Enter, Tab, ., ,, ;, :, !, ?).
    Boundary,
    /// Backspace.
    Backspace,
    /// Modifier alone, NumLock, etc — ignore.
    Discard,
    /// Esc, arrows, Home/End, … — abandon the buffer.
    EndAndDiscard,
}

/// Classify a Win-SC1 scancode into a [`KeyKind`]. Same scancodes are
/// produced (after normalisation) on macOS / Linux backends, so this
/// table is OS-agnostic.
fn classify(scancode: u32) -> KeyKind {
    match scancode {
        // Esc
        0x01 => KeyKind::EndAndDiscard,
        // Number row 1..=0 + - =
        0x02..=0x0D => KeyKind::Word,
        // Backspace
        0x0E => KeyKind::Backspace,
        // Tab
        0x0F => KeyKind::Boundary,
        // QWERTY row + [ ]
        0x10..=0x1B => KeyKind::Word,
        // Enter
        0x1C => KeyKind::Boundary,
        // Ctrl-L
        0x1D => KeyKind::Discard,
        // ASDF row + ; '
        0x1E..=0x28 => KeyKind::Word,
        // Backtick
        0x29 => KeyKind::Word,
        // Shift-L
        0x2A => KeyKind::Discard,
        // Backslash + ZXCV row
        0x2B..=0x32 => KeyKind::Word,
        // , . /  — boundaries (sentence punctuation)
        0x33..=0x35 => KeyKind::Boundary,
        // Shift-R
        0x36 => KeyKind::Discard,
        // Numpad ops
        0x37 => KeyKind::Word,
        // Alt-L
        0x38 => KeyKind::Discard,
        // Spacebar
        0x39 => KeyKind::Boundary,
        // Caps Lock + F1..F10
        0x3A..=0x44 => KeyKind::Discard,
        // Num Lock, Scroll Lock, Numpad area
        0x45..=0x53 => KeyKind::Discard,
        _ => KeyKind::Discard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kb_types::Modifiers;

    fn press(sc: u32) -> KeyEvent {
        KeyEvent {
            vk: 0,
            scancode: sc,
            direction: KeyDirection::Press,
            modifiers: Modifiers::NONE,
            injected: false,
            timestamp_ms: 0,
        }
    }

    #[test]
    fn space_completes_word() {
        let mut b = WordBuffer::new();
        assert_eq!(b.feed(press(0x23)), WordBoundary::InProgress); // h
        assert_eq!(b.feed(press(0x12)), WordBoundary::InProgress); // e
        assert_eq!(
            b.feed(press(0x39)),
            WordBoundary::WordCompleted {
                boundary_scancode: 0x39,
                boundary_shift: false,
            }
        );
    }

    #[test]
    fn backspace_pops() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23));
        b.feed(press(0x12));
        b.feed(press(0x0E)); // backspace
        assert_eq!(b.keys().len(), 1);
    }

    #[test]
    fn esc_clears_buffer() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23));
        b.feed(press(0x01));
        assert_eq!(b.keys().len(), 0);
    }
}
