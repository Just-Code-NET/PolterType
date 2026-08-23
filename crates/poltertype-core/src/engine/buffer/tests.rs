use poltertype_input::{KeyDirection, KeyEvent};
use poltertype_types::SC_POINTER_BUTTON;

use super::*;
use poltertype_types::Modifiers;

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

/// Feed a lowercase Latin-ish word key (letter hint on).
fn word_key(b: &mut WordBuffer, sc: u32, ch: char) -> WordBoundary {
    b.feed(press(sc), Some(ch), true)
}

fn space(b: &mut WordBuffer) -> WordBoundary {
    b.feed(press(0x39), Some(' '), false)
}

fn backspace(b: &mut WordBuffer) -> WordBoundary {
    b.feed(press(0x0E), None, false)
}

#[test]
fn space_completes_word() {
    let mut b = WordBuffer::new();
    assert_eq!(
        b.feed(press(0x23), Some('h'), true),
        WordBoundary::InProgress
    );
    assert_eq!(
        b.feed(press(0x12), Some('e'), true),
        WordBoundary::InProgress
    );
    let WordBoundary::WordCompleted {
        boundary_scancode,
        tainted,
        ..
    } = b.feed(press(0x39), Some(' '), false)
    else {
        panic!("expected WordCompleted");
    };
    assert_eq!(boundary_scancode, 0x39);
    assert!(!tainted);
    assert_eq!(b.completed().len(), 2);
}

#[test]
fn backspace_pops() {
    let mut b = WordBuffer::new();
    b.feed(press(0x23), Some('h'), true);
    b.feed(press(0x12), Some('e'), true);
    b.feed(press(0x0E), None, false); // Backspace
    assert_eq!(b.keys().len(), 1);
    assert!(!b.poisoned());
}

#[test]
fn esc_abandons_and_taints_midword() {
    let mut b = WordBuffer::new();
    b.feed(press(0x23), Some('h'), true);
    assert_eq!(b.feed(press(0x01), None, false), WordBoundary::Abandoned);
    assert_eq!(b.keys().len(), 0);
    // Mid-word abandon taints the *next* completion…
    word_key(&mut b, 0x23, 'h');
    let WordBoundary::WordCompleted { tainted, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(tainted, "completion after mid-word Esc must be tainted");
    // …and the one after that is trusted again.
    word_key(&mut b, 0x23, 'h');
    let WordBoundary::WordCompleted { tainted, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(!tainted);
}

#[test]
fn esc_between_words_does_not_taint() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x23, 'h');
    space(&mut b);
    assert_eq!(b.feed(press(0x01), None, false), WordBoundary::Abandoned);
    word_key(&mut b, 0x12, 'e');
    let WordBoundary::WordCompleted { tainted, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(
        !tainted,
        "Esc with no word in progress must not taint the next word"
    );
}

/// The heart of the "broken word" report: type a word, hit space,
/// backspace over the space and a couple of letters, retype them. The
/// buffer must re-open the previous word so the engine's backspace count
/// covers the WHOLE word on screen, not just the retyped tail.
#[test]
fn backspace_over_boundary_reopens_previous_word() {
    let mut b = WordBuffer::new();
    // "ghbdsn" — привіт mistyped in en-US.
    for (sc, ch) in [
        (0x22u32, 'g'),
        (0x23, 'h'),
        (0x30, 'b'),
        (0x20, 'd'),
        (0x1F, 's'),
        (0x31, 'n'),
    ] {
        word_key(&mut b, sc, ch);
    }
    space(&mut b);
    assert_eq!(b.keys().len(), 0);
    assert_eq!(b.completed().len(), 6);

    backspace(&mut b); // over the space
    assert_eq!(b.keys().len(), 6, "re-open must restore the full word");
    assert!(!b.poisoned());

    backspace(&mut b); // eat "sn"
    backspace(&mut b);
    assert_eq!(b.keys().len(), 4);

    word_key(&mut b, 0x1F, 's');
    word_key(&mut b, 0x31, 'n');
    let WordBoundary::WordCompleted { tainted, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(!tainted);
    assert_eq!(
        b.completed().len(),
        6,
        "completion after edit must cover the whole on-screen word"
    );
}

/// Double space after a word, then backspace through both spaces:
/// the word re-opens only after the *entire* boundary run is gone.
#[test]
fn boundary_run_tracks_multiple_separators() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x23, 'h');
    word_key(&mut b, 0x12, 'e');
    space(&mut b);
    space(&mut b); // second separator
    backspace(&mut b);
    assert_eq!(b.keys().len(), 0, "one separator still on screen");
    backspace(&mut b);
    assert_eq!(b.keys().len(), 2, "now the word is re-opened");
    assert!(!b.poisoned());
}

/// Backspacing past the start of everything we track loses the
/// *context*, not the next word: what sits left of the caret is
/// unknown, so no suggestion — but a word typed afterwards is observed
/// from its first key and stays correctable. Regression: a line rubbed
/// out with Backspace left the engine mute for everything retyped.
#[test]
fn backspace_into_unknown_text_keeps_the_next_word_correctable() {
    let mut b = WordBuffer::new();
    backspace(&mut b); // nothing tracked at all
    assert!(!b.poisoned());
    word_key(&mut b, 0x23, 'h');
    let WordBoundary::WordCompleted {
        tainted,
        started_clean,
        ..
    } = space(&mut b)
    else {
        panic!("expected WordCompleted");
    };
    assert!(!tainted, "the word itself was fully observed");
    assert!(!started_clean, "what precedes it was not");
    // The boundary re-syncs everything: the word after it is clean.
    word_key(&mut b, 0x12, 'e');
    let WordBoundary::WordCompleted { started_clean, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(started_clean);
}

/// Deleting the re-opened word completely and then continuing to
/// backspace walks into unknown text: tracking restarts and the caller
/// is told, but nothing was in flight to lose.
#[test]
fn deleting_past_reopened_word_abandons() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x23, 'h');
    space(&mut b);
    backspace(&mut b); // re-open "h"
    assert_eq!(b.keys().len(), 1);
    backspace(&mut b); // delete "h"
    assert_eq!(b.keys().len(), 0);
    assert!(!b.poisoned(), "still exactly at tracked ground zero");
    assert_eq!(backspace(&mut b), WordBoundary::Abandoned);
    assert!(!b.poisoned());
}

/// A tainted completion must not leave the word re-openable — the
/// stash was unreliable by definition.
#[test]
fn tainted_completion_is_not_reopenable() {
    let mut b = WordBuffer::new();
    b.poison();
    word_key(&mut b, 0x23, 'h');
    let WordBoundary::WordCompleted { tainted, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(tainted);
    assert_eq!(backspace(&mut b), WordBoundary::Abandoned); // over the space
    assert_eq!(b.keys().len(), 0, "would re-open if stashed — must not");
}

/// Arrow keys (evdev codes) abandon the word and taint the next
/// completion — the caret moved mid-word.
#[test]
fn arrow_key_midword_taints_next_completion() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x23, 'h');
    assert_eq!(
        b.feed(press(105), None, false), // KEY_LEFT
        WordBoundary::Abandoned
    );
    word_key(&mut b, 0x12, 'e');
    let WordBoundary::WordCompleted { tainted, .. } = space(&mut b) else {
        panic!("expected WordCompleted");
    };
    assert!(tainted);
}

/// Mouse click mid-word behaves like navigation.
#[test]
fn pointer_click_abandons_word() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x23, 'h');
    assert_eq!(
        b.feed(press(SC_POINTER_BUTTON), None, false),
        WordBoundary::Abandoned
    );
    assert_eq!(b.keys().len(), 0);
}

/// Numpad Enter (evdev 96) is a boundary, not a discard.
#[test]
fn numpad_enter_completes_word() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x23, 'h');
    assert!(matches!(
        b.feed(press(0x60), None, false),
        WordBoundary::WordCompleted { .. }
    ));
}

/// Regression: scancode 0x33 is `,` under en-US (boundary) but the
/// letter `б` under uk-UA (word character). Classifying by scancode
/// alone cut `будь` to `удь` and then auto-switched it to `elm`; the
/// cross-layout-letter hint (`true` here) is what keeps the buffer
/// whole.
#[test]
fn classifies_by_produced_char_not_scancode() {
    let mut b = WordBuffer::new();
    // "будь" under uk-UA: the same scancodes as ",elm" under en-US.
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

/// The deliberate cost of the cross-layout hint: a genuine comma typed
/// under en-US stays in the buffer, because its scancode is `б`
/// somewhere. Keeping cross-script words intact wins, and a
/// never-a-letter boundary (Space / Enter / Tab) still ends the word.
#[test]
fn cross_layout_letter_scancode_absorbs_punct_under_current_layout() {
    let mut b = WordBuffer::new();
    b.feed(press(0x23), Some('h'), true);
    b.feed(press(0x12), Some('e'), true);
    assert_eq!(
        b.feed(press(0x33), Some(','), true),
        WordBoundary::InProgress
    );
    assert_eq!(b.keys().len(), 3);
    assert!(matches!(
        b.feed(press(0x39), Some(' '), false),
        WordBoundary::WordCompleted { .. }
    ));
}

#[test]
fn apostrophe_keeps_word_intact() {
    let mut b = WordBuffer::new();
    // "don't" — 0x28 is `'` in en-US but `є` in uk-UA, so the apostrophe
    // is absorbed via the cross-layout path rather than `is_word_char`.
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
    // "ім'я" — typographic apostrophe variants share the `is_word_char`
    // bucket with the ASCII one.
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
    // 0x35 is `/` under en-US but `.` under uk-UA, and neither is a
    // letter anywhere — so the produced-char path decides.
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

/// 0x35 is `/` under en-US. The word it opens has to carry that
/// separator to the decision — `/tmp` is a path, `tmp` alone is not.
#[test]
fn word_remembers_the_separator_it_opened_after() {
    let mut b = WordBuffer::new();
    assert_eq!(
        b.feed(press(0x35), Some('/'), false),
        WordBoundary::InProgress
    );
    word_key(&mut b, 0x14, 't');
    word_key(&mut b, 0x32, 'm');
    space(&mut b);
    assert_eq!(b.completed_lead(), Some((0x35, false)));
    // The next word opens after the space, not after the slash.
    word_key(&mut b, 0x14, 't');
    space(&mut b);
    assert_eq!(b.completed_lead(), Some((0x39, false)));
}

#[test]
fn first_word_of_the_buffer_has_no_separator() {
    let mut b = WordBuffer::new();
    word_key(&mut b, 0x14, 't');
    space(&mut b);
    assert_eq!(b.completed_lead(), None);
}

/// Backspacing over the separator re-opens the previous word, which
/// must come back with its own lead rather than the one it was
/// re-opened across.
#[test]
fn reopened_word_restores_its_own_separator() {
    let mut b = WordBuffer::new();
    b.feed(press(0x35), Some('/'), false);
    word_key(&mut b, 0x14, 't');
    space(&mut b);
    backspace(&mut b); // deletes the space, re-opens `t`
    word_key(&mut b, 0x32, 'm');
    space(&mut b);
    assert_eq!(b.completed_lead(), Some((0x35, false)));
}

/// A caret that moved takes the separator with it: what sits left of
/// the word is no longer something the buffer saw.
#[test]
fn caret_move_forgets_the_separator() {
    let mut b = WordBuffer::new();
    b.feed(press(0x35), Some('/'), false);
    b.feed(press(SC_POINTER_BUTTON), None, false);
    word_key(&mut b, 0x14, 't');
    word_key(&mut b, 0x32, 'm');
    space(&mut b);
    assert_eq!(b.completed_lead(), None);
}
