use crate::logsafe;

#[test]
fn redaction_hides_the_word_and_keeps_its_length() {
    assert_eq!(logsafe::render("mañana", false), "<6 chars>");
    assert_eq!(logsafe::render("привіт", false), "<6 chars>");
    assert_eq!(logsafe::render("", false), "<0 chars>");
}

#[test]
fn opted_in_debug_shows_the_word_in_backticks() {
    assert_eq!(logsafe::render("mañana", true), "`mañana`");
}
