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
/// first and decisively (layout-independent). For data keys we use
/// either the cross-layout "is letter somewhere" hint (covers users
/// typing a Cyrillic word while en-US is active) or — if the
/// scancode isn't ever a letter in any known layout — the actual
/// character the *current* layout produced.
fn classify(scancode: u32, produced: Option<char>, letter_in_any_layout: bool) -> KeyKind {
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

    // ---- Cross-layout letter hint ----------------------------------
    // If this scancode produces a letter in *any* known keyboard
    // layout, treat it as part of a word — even if the *current*
    // layout would render it as punctuation. Catches Cyrillic words
    // typed while en-US is active (e.g. `ж`-position renders as `;`).
    if letter_in_any_layout {
        return KeyKind::Word;
    }

    // ---- Otherwise classify by current-layout's produced char -------
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
        assert_eq!(b.feed(press(0x23), Some('h'), true), WordBoundary::InProgress);
        assert_eq!(b.feed(press(0x12), Some('e'), true), WordBoundary::InProgress);
        let WordBoundary::WordCompleted {
            boundary_scancode, ..
        } = b.feed(press(0x39), Some(' '), false)
        else {
            panic!("expected WordCompleted");
        };
        assert_eq!(boundary_scancode, 0x39);
    }

    #[test]
    fn backspace_pops() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'), true);
        b.feed(press(0x12), Some('e'), true);
        b.feed(press(0x0E), None, false); // Backspace
        assert_eq!(b.keys().len(), 1);
    }

    #[test]
    fn esc_clears_buffer() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'), true);
        b.feed(press(0x01), None, false); // Esc
        assert_eq!(b.keys().len(), 0);
    }

    /// Regression: scancode 0x33 is `,` under en-US (boundary) but
    /// the letter `б` under uk-UA (word character). Earlier versions
    /// classified by scancode alone and silently dropped `б` /
    /// reset the word — Cyrillic words like `будь` got cut to `удь`
    /// and then auto-switched to `elm`. The cross-layout-letter hint
    /// (`true` here) is what keeps the buffer whole.
    #[test]
    fn classifies_by_produced_char_not_scancode() {
        let mut b = WordBuffer::new();
        // Simulate typing "будь" under uk-UA: scancodes the same as
        // ",elm" under en-US, but the produced characters differ.
        assert_eq!(
            b.feed(press(0x33), Some('б'), true),
            WordBoundary::InProgress
        );
        assert_eq!(
            b.feed(press(0x12), Some('у'), true),
            WordBoundary::InProgress
        );
        assert_eq!(
            b.feed(press(0x26), Some('д'), true),
            WordBoundary::InProgress
        );
        assert_eq!(
            b.feed(press(0x32), Some('ь'), true),
            WordBoundary::InProgress
        );
        assert_eq!(b.keys().len(), 4);
    }

    /// The flip side of the cross-layout hint: a scancode that's `,`
    /// under en-US but `б` under uk-UA (`letter_in_any_layout = true`)
    /// is treated as a word character even when the *current* layout
    /// produced a comma. That's the deliberate trade-off — keeping
    /// cross-script words intact wins; the comma stays in the buffer
    /// and a never-letter boundary (Space / Enter / Tab) is what
    /// actually ends the word.
    #[test]
    fn cross_layout_letter_scancode_absorbs_punct_under_current_layout() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'), true);
        b.feed(press(0x12), Some('e'), true);
        // 0x33 produces `,` under en-US but is `б` under uk-UA —
        // hint says `true`, so it's absorbed as a word character.
        assert_eq!(
            b.feed(press(0x33), Some(','), true),
            WordBoundary::InProgress
        );
        assert_eq!(b.keys().len(), 3);
        // Space (never a letter anywhere) is what actually ends the
        // word.
        assert!(matches!(
            b.feed(press(0x39), Some(' '), false),
            WordBoundary::WordCompleted { .. }
        ));
    }

    #[test]
    fn apostrophe_keeps_word_intact() {
        let mut b = WordBuffer::new();
        // "don't" — 0x28 is `'` in en-US but `є` in uk-UA, so the
        // cross-layout hint is `true` and the apostrophe is absorbed
        // as a word character via the cross-layout path.
        b.feed(press(0x20), Some('d'), true);
        b.feed(press(0x18), Some('o'), true);
        b.feed(press(0x31), Some('n'), true);
        b.feed(press(0x28), Some('\''), true);
        b.feed(press(0x14), Some('t'), true);
        assert_eq!(b.keys().len(), 5);
    }

    #[test]
    fn ukrainian_apostrophe_keeps_word_intact() {
        let mut b = WordBuffer::new();
        // "ім'я" — typographic apostrophe variants land in the same
        // is_word_char bucket.
        b.feed(press(0x17), Some('і'), true);
        b.feed(press(0x32), Some('м'), true);
        b.feed(press(0x28), Some('\u{2019}'), true);
        b.feed(press(0x2C), Some('я'), true);
        assert_eq!(b.keys().len(), 4);
    }

    #[test]
    fn period_ends_word_in_either_layout() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'), true);
        b.feed(press(0x12), Some('e'), true);
        // 0x35 is `/` under en-US but `.` under uk-UA — neither is
        // a letter anywhere, so `letter_in_any_layout = false` and
        // the produced-char path classifies `.` as a boundary.
        assert!(matches!(
            b.feed(press(0x35), Some('.'), false),
            WordBoundary::WordCompleted { .. }
        ));
    }

    #[test]
    fn unmapped_scancode_is_discarded_not_boundary() {
        let mut b = WordBuffer::new();
        b.feed(press(0x23), Some('h'), true);
        b.feed(press(0x12), Some('e'), true);
        // Some exotic OEM scancode the layout doesn't map.
        b.feed(press(0x56), None, false);
        assert_eq!(b.keys().len(), 2);
    }
}
