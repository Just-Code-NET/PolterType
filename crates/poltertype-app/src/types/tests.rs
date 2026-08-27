//! The deferred-word list's rules, which are all about *not* growing.

use super::DeferredWords;
use poltertype_types::LayoutId;

fn en() -> LayoutId {
    LayoutId::from("en-US")
}

fn uk() -> LayoutId {
    LayoutId::from("uk-UA")
}

fn words(d: &DeferredWords) -> Vec<String> {
    d.iter().map(|(_, w)| w.clone()).collect()
}

#[test]
fn newest_offer_comes_first() {
    let mut d = DeferredWords::new();
    d.push(en(), "first".into());
    d.push(en(), "second".into());
    assert_eq!(words(&d), ["second", "first"]);
}

/// The same word missed twice is the same word. Two rows for it would
/// read as two different offers and cost a slot that a different word
/// could have had.
#[test]
fn a_repeat_moves_to_the_front_instead_of_duplicating() {
    let mut d = DeferredWords::new();
    d.push(en(), "alpha".into());
    d.push(en(), "beta".into());
    d.push(en(), "alpha".into());
    assert_eq!(words(&d), ["alpha", "beta"]);
}

/// Same spelling, different layout, is a different entry: it goes into
/// a different wordlist, and can be a word in one and gibberish in the
/// other.
#[test]
fn the_same_spelling_in_two_layouts_is_two_entries() {
    let mut d = DeferredWords::new();
    d.push(en(), "cnjk".into());
    d.push(uk(), "cnjk".into());
    assert_eq!(words(&d), ["cnjk", "cnjk"]);
    assert!(d.take(&uk(), "cnjk"));
    assert_eq!(words(&d), ["cnjk"]);
}

/// The cap is the whole privacy argument: this is the one place the app
/// holds words the user typed beyond the engine's single-word buffer,
/// so it has to stay a menu rather than become a history.
#[test]
fn the_list_is_bounded() {
    let mut d = DeferredWords::new();
    for i in 0..40 {
        d.push(en(), format!("word{i}"));
    }
    assert_eq!(words(&d).len(), DeferredWords::CAP);
    assert_eq!(words(&d)[0], "word39", "and it keeps the newest");
}

#[test]
fn taking_a_word_that_is_not_there_changes_nothing() {
    let mut d = DeferredWords::new();
    d.push(en(), "here".into());
    assert!(!d.take(&en(), "absent"));
    assert_eq!(words(&d), ["here"]);
}

/// An empty offer would be an unclickable row that adds nothing.
#[test]
fn blank_words_are_not_kept() {
    let mut d = DeferredWords::new();
    d.push(en(), "   ".into());
    d.push(en(), String::new());
    assert!(d.is_empty());
}
