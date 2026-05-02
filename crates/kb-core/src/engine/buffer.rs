//! Word buffer: accumulate keystrokes until the user finishes a word.
//!
//! "Finishes" = the user produces a key whose **character** under the
//! current layout is a word boundary (whitespace, sentence
//! punctuation, brackets, …). Backspace also deletes the most recent
//! buffered key.
//!
//! Why we look at the *character*, not the raw scancode: the same
//! physical key produces wildly different things across layouts.
//! Scancode `0x33` is `,` (a boundary) under en-US but the letter
//! `б` (a word character) under uk-UA — and we want `б` to land in
//! the buffer so words like `будь` are detected correctly. Earlier
//! versions of this module classified by scancode alone and silently
//! split Cyrillic words at every `б` / `ю` / similar.

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

    /// Feed a [`KeyEvent`] together with the character it produces
    /// under the **currently active** OS layout. Pass `None` if the
    /// scancode has no mapping (control / function keys); the
    /// classifier falls back to its scancode-only rules for those.
    pub fn feed(&mut self, ev: KeyEvent, produced: Option<char>) -> WordBoundary {
        if ev.direction != KeyDirection::Press {
            return WordBoundary::InProgress;
        }

        match classify(ev.scancode, produced) {
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
            KeyKind::Discard => WordBoundary::InProgress,
            KeyKind::EndAndDiscard => {
                self.keys.clear();
                WordBoundary::InProgress
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    /// Letter / digit / apostrophe / hyphen — accumulate.
    Word,
    /// Whitespace / sentence punctuation / brackets — end the word.
    Boundary,
    /// Backspace.
    Backspace,
    /// Modifier alone, NumLock, function key — ignore.
    Discard,
    /// Esc, arrows, Home/End, navigation — abandon the buffer.
    EndAndDiscard,
}

/// Classify the keystroke. Control / structural scancodes are matched
/// first and decisively (they're the same on every layout). For
/// everything else we look at the *character* the layout produced.
fn classify(scancode: u32, produced: Option<char>) -> KeyKind {
    // ---- Control / structural keys: layout-independent --------------
    match scancode {
        // Esc — abandon.
        0x01 => return KeyKind::EndAndDiscard,
        // Backspace.
        0x0E => return KeyKind::Backspace,
        // Tab.
        0x0F => return KeyKind::Boundary,
        // Enter.
        0x1C => return KeyKind::Boundary,
        // Spacebar.
        0x39 => return KeyKind::Boundary,
        // Modifiers / Caps Lock / Function row / Numpad NumLock
        // / Scroll Lock — ignore but stay inside the word.
        0x1D | 0x2A | 0x36 | 0x38 | 0x3A => return KeyKind::Discard,
        // F1..F10 + numpad cluster + extended (arrows / Home / End /
        // PageUp / PageDown / Insert / Delete arrive as 0x47..=0x53
        // when extended-prefixed). Treat as nav → end + drop.
        0x3B..=0x53 => return KeyKind::EndAndDiscard,
        _ => {}
    }

    // ---- Data keys: classify by produced character ------------------
    let Some(ch) = produced else {
        // Scancode isn't in the layout's mapping table (e.g. exotic
        // OEM keys we don't track). Discard but don't abort the word.
        return KeyKind::Discard;
    };

    if is_word_char(ch) {
        KeyKind::Word
    } else {
        KeyKind::Boundary
    }
}

/// Characters that count as part of a word for the engine: all
/// alphabetic Unicode (Latin, Cyrillic, Greek, …), digits, and the
/// few punctuation marks that appear *inside* words (apostrophe in
/// `don't` / `ім'я`, hyphen in `well-known`).
fn is_word_char(ch: char) -> bool {
    ch.is_alphabetic() || ch.is_ascii_digit() || matches!(ch, '\'' | 'ʼ' | '\u{2019}' | '-')
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
        assert_eq!(b.feed(press(0x23), Some('h')), WordBoundary::InProgress);
        assert_eq!(b.feed(press(0x12), Some('e')), WordBoundary::InProgress);
        let WordBoundary::WordCompleted {
            boundary_scancode, ..
        } = b.feed(press(0x39), Some(' '))
        else {
            panic!("expected WordCompleted");
        };
        assert_eq!(boundary_scancode, 0x39);
    }

    #[test]
    fn backspace_pops() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'));
        b.feed(press(0x12), Some('e'));
        b.feed(press(0x0E), None); // Backspace
        assert_eq!(b.keys().len(), 1);
    }

    #[test]
    fn esc_clears_buffer() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'));
        b.feed(press(0x01), None); // Esc
        assert_eq!(b.keys().len(), 0);
    }

    /// Regression: scancode 0x33 is `,` under en-US (boundary) but
    /// the letter `б` under uk-UA (word character). Earlier versions
    /// classified by scancode alone and silently dropped `б` /
    /// reset the word — Cyrillic words like `будь` got cut to `удь`
    /// and then auto-switched to `elm`.
    #[test]
    fn classifies_by_produced_char_not_scancode() {
        let mut b = WordBuffer::new();
        // Simulate typing "будь" under uk-UA: scancodes the same as
        // ",elm" under en-US, but the produced characters differ.
        assert_eq!(b.feed(press(0x33), Some('б')), WordBoundary::InProgress);
        assert_eq!(b.feed(press(0x12), Some('у')), WordBoundary::InProgress);
        assert_eq!(b.feed(press(0x26), Some('д')), WordBoundary::InProgress);
        assert_eq!(b.feed(press(0x32), Some('ь')), WordBoundary::InProgress);
        assert_eq!(b.keys().len(), 4);
    }

    #[test]
    fn comma_under_en_is_boundary() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'));
        b.feed(press(0x12), Some('e'));
        // Same 0x33 scancode but produces `,` under en-US.
        assert!(matches!(
            b.feed(press(0x33), Some(',')),
            WordBoundary::WordCompleted { .. }
        ));
    }

    #[test]
    fn apostrophe_keeps_word_intact() {
        let mut b = WordBuffer::new();
        // "don't"
        b.feed(press(0x20), Some('d'));
        b.feed(press(0x18), Some('o'));
        b.feed(press(0x31), Some('n'));
        b.feed(press(0x28), Some('\''));
        b.feed(press(0x14), Some('t'));
        assert_eq!(b.keys().len(), 5);
    }

    #[test]
    fn ukrainian_apostrophe_keeps_word_intact() {
        let mut b = WordBuffer::new();
        // "ім'я" — typographic apostrophe variants land in the same
        // is_word_char bucket.
        b.feed(press(0x17), Some('і'));
        b.feed(press(0x32), Some('м'));
        b.feed(press(0x28), Some('\u{2019}'));
        b.feed(press(0x2C), Some('я'));
        assert_eq!(b.keys().len(), 4);
    }

    #[test]
    fn period_ends_word_in_either_layout() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'));
        b.feed(press(0x12), Some('e'));
        // 0x35 is `/` under en-US but `.` under uk-UA — both are
        // boundaries because both are non-word characters.
        assert!(matches!(
            b.feed(press(0x35), Some('.')),
            WordBoundary::WordCompleted { .. }
        ));
    }

    #[test]
    fn unmapped_scancode_is_discarded_not_boundary() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'));
        b.feed(press(0x12), Some('e'));
        // Some exotic OEM scancode the layout doesn't map.
        b.feed(press(0x56), None);
        assert_eq!(b.keys().len(), 2);
    }
}
