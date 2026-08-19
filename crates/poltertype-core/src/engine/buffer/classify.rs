//! Raw key event → `KeyKind` classification.

use super::*;
use poltertype_types::SC_POINTER_BUTTON;

/// Classify the keystroke. Control / structural scancodes are matched
/// first and decisively (layout-independent). Data keys go by the
/// cross-layout "is letter somewhere" hint, and only when that says no
/// by the character the *current* layout produced.
pub(crate) fn classify(
    scancode: u32,
    produced: Option<char>,
    letter_in_any_layout: bool,
) -> KeyKind {
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
        // Numpad Enter as Linux evdev reports it (KEY_KPENTER = 96).
        // In the Discard bucket a word "ended" with numpad-Enter
        // silently ran into the next one and corrupted its correction.
        0x60 => return KeyKind::Boundary,
        // Linux evdev navigation cluster, KEY_HOME=102 … KEY_DELETE=111.
        // On Windows the same SC-1 codes sit in the exotic F16+ range,
        // which never occurs in normal typing, so this is safe
        // cross-platform.
        0x66..=0x6F => return KeyKind::EndAndDiscard,
        // Pointer button (mouse click / touchpad tap) — the listener
        // reports it with this pseudo-scancode. Caret / focus moved.
        SC_POINTER_BUTTON => return KeyKind::EndAndDiscard,
        _ => {}
    }

    // A letter in *any* known layout is part of a word even where the
    // current layout renders it as punctuation — that is what keeps a
    // Cyrillic word typed under en-US whole (`ж` renders as `;`).
    if letter_in_any_layout {
        return KeyKind::Word;
    }

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
pub(crate) fn is_word_char(ch: char) -> bool {
    ch.is_alphabetic() || ch.is_ascii_digit() || matches!(ch, '\'' | 'ʼ' | '\u{2019}' | '-')
}
