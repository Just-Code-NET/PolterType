use super::*;
use crate::commands::CommandAction;

fn command(trigger: &str) -> UserCommand {
    UserCommand {
        id: "t".into(),
        name: String::new(),
        trigger: trigger.into(),
        action: CommandAction::TypeText { text: "x".into() },
        apps: Vec::new(),
    }
}

fn history(words: &[&str]) -> WordHistory {
    let mut h = WordHistory::default();
    for w in words {
        h.push(w);
    }
    h
}

// ── the bounded history ──────────────────────────────────────────────

#[test]
fn history_keeps_only_the_most_recent_words() {
    let h = history(&["one", "two", "three", "four", "five", "six"]);
    assert_eq!(h.len(), MAX_HISTORY_WORDS);
    assert_eq!(h.tail(2), ["five", "six"]);
}

#[test]
fn asking_for_more_than_exists_returns_what_there_is() {
    let h = history(&["only"]);
    assert_eq!(h.tail(3), ["only"]);
}

#[test]
fn clearing_forgets_everything() {
    let mut h = history(&["a", "b"]);
    h.clear();
    assert!(h.is_empty());
    assert!(h.tail(2).is_empty());
}

#[test]
fn empty_words_are_not_recorded() {
    let mut h = WordHistory::default();
    h.push("");
    assert!(h.is_empty());
}

// ── matching ─────────────────────────────────────────────────────────

/// The pre-existing behaviour, unchanged: a one-word trigger ignores
/// history entirely.
#[test]
fn a_single_token_trigger_matches_on_the_current_word_alone() {
    let c = command("anrl");
    assert!(phrase_matches(&c, &WordHistory::default(), "anrl"));
    assert!(phrase_matches(&c, &history(&["random", "words"]), "anrl"));
    assert!(!phrase_matches(&c, &WordHistory::default(), "anr"));
}

#[test]
fn a_two_token_trigger_needs_the_preceding_word() {
    let c = command("best regards");
    assert!(phrase_matches(&c, &history(&["best"]), "regards"));
    assert!(!phrase_matches(&c, &history(&["kind"]), "regards"));
    assert!(
        !phrase_matches(&c, &WordHistory::default(), "regards"),
        "no history means no phrase"
    );
}

#[test]
fn a_three_token_trigger_needs_both_preceding_words_in_order() {
    let c = command("with best regards");
    assert!(phrase_matches(&c, &history(&["with", "best"]), "regards"));
    assert!(
        !phrase_matches(&c, &history(&["best", "with"]), "regards"),
        "order matters"
    );
    assert!(
        !phrase_matches(&c, &history(&["best"]), "regards"),
        "a truncated prefix must not match"
    );
}

/// Words further back must not interfere: the trigger's tokens have
/// to be the *immediately* preceding ones.
#[test]
fn intervening_words_break_the_phrase() {
    let c = command("best regards");
    assert!(!phrase_matches(
        &c,
        &history(&["best", "sincerely"]),
        "regards"
    ));
}

#[test]
fn matching_stays_case_sensitive() {
    let c = command("best regards");
    assert!(!phrase_matches(&c, &history(&["Best"]), "regards"));
    assert!(!phrase_matches(&c, &history(&["best"]), "Regards"));
}

#[test]
fn a_whitespace_only_trigger_matches_nothing() {
    let c = command("   ");
    assert!(!phrase_matches(&c, &history(&["a"]), ""));
    assert!(!phrase_matches(&c, &history(&["a"]), "b"));
}

#[test]
fn repeated_separators_in_a_trigger_are_one_separator() {
    let c = command("best   regards");
    assert!(
        phrase_matches(&c, &history(&["best"]), "regards"),
        "a trigger typed with two spaces should still match one"
    );
}

// ── erasing ──────────────────────────────────────────────────────────

/// A single-token trigger erases what it always did: the buffered
/// keys plus the boundary character.
#[test]
fn a_single_token_erases_word_plus_boundary() {
    assert_eq!(erase_len(&command("anrl"), 4), 5);
}

/// A phrase has to take back the earlier words and the space after
/// each, or half the trigger is left on screen.
#[test]
fn a_phrase_erases_the_earlier_words_and_their_separators() {
    // "best regards " → 7 keys of "regards", +1 boundary,
    // +5 for "best" and its space.
    assert_eq!(erase_len(&command("best regards"), 7), 13);
}

#[test]
fn erase_length_counts_characters_not_bytes() {
    // Cyrillic tokens are two bytes per character; the screen shows
    // one glyph each and a backspace removes one glyph.
    assert_eq!(erase_len(&command("з повагою"), 8), 9 + 2);
}

// ── focus scoping ────────────────────────────────────────────────────

/// The engine matches *before* recording the word just completed, so
/// these mirror that order: only the earlier words go into the
/// history, and the last token is passed as the current word.
///
/// Half a trigger typed in one window must not combine with a word
/// typed in another.
#[test]
fn changing_application_drops_the_history() {
    let mut h = WordHistory::default();
    h.push_in(Some("kate"), "best");
    // Focus moved; the next word arrives from a different app.
    h.push_in(Some("firefox"), "and");
    assert!(
        !phrase_matches(&command("best regards"), &h, "regards"),
        "the `best` typed in kate must not complete a phrase in firefox"
    );
}

#[test]
fn staying_in_one_application_keeps_the_history() {
    let mut h = WordHistory::default();
    h.push_in(Some("kate"), "best");
    assert!(phrase_matches(&command("best regards"), &h, "regards"));
}

/// Where focus tracking does not answer — macOS, most terminals on
/// GNOME/KDE — every word arrives with `None`, which is one context
/// rather than none. The feature keeps working there.
#[test]
fn an_unknown_context_is_still_one_context() {
    let mut h = WordHistory::default();
    h.push_in(None, "best");
    assert!(phrase_matches(&command("best regards"), &h, "regards"));
}
