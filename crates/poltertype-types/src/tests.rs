use crate::logsafe;
use crate::{ModifierKey, Modifiers};

/// The bug this guards: a low-level hook that reads the keyboard as it
/// was *before* the event it is delivering reports "Ctrl held" on the
/// Ctrl release, and nothing corrects that until the next keystroke —
/// so the engine waited for a release it had already been handed.
#[test]
fn a_release_seen_through_a_pre_event_snapshot_clears_the_modifier() {
    let before = Modifiers {
        control: true,
        ..Modifiers::NONE
    };
    let after = before.after_transition(ModifierKey::Control, false, false);
    assert_eq!(after, Modifiers::NONE);
}

#[test]
fn a_press_seen_through_a_pre_event_snapshot_sets_the_modifier() {
    let after = Modifiers::NONE.after_transition(ModifierKey::Shift, true, false);
    assert!(after.shift);
    assert!(!after.control && !after.alt && !after.meta);
}

#[test]
fn releasing_one_side_keeps_the_modifier_while_the_other_side_is_held() {
    let before = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    assert!(
        before
            .after_transition(ModifierKey::Shift, false, true)
            .shift
    );
    assert!(
        !before
            .after_transition(ModifierKey::Shift, false, false)
            .shift
    );
}

#[test]
fn the_transition_touches_only_its_own_modifier() {
    let before = Modifiers {
        control: true,
        alt: true,
        caps: true,
        ..Modifiers::NONE
    };
    let after = before.after_transition(ModifierKey::Meta, true, false);
    assert_eq!(
        after,
        Modifiers {
            meta: true,
            ..before
        }
    );
    // Caps is a latch, not a held key; a modifier event never moves it.
    assert!(after.caps);
}

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

#[test]
fn lam_alef_folds_and_unfolds_each_form() {
    for (alef, form) in [
        ('\u{0627}', '\u{FEFB}'),
        ('\u{0623}', '\u{FEF7}'),
        ('\u{0625}', '\u{FEF9}'),
        ('\u{0622}', '\u{FEF5}'),
    ] {
        assert_eq!(crate::fold_lam_alef(&[0x0644, alef as u16]), Some(form));
        assert_eq!(crate::lam_alef_pair(form), Some(['\u{0644}', alef]));
    }
    assert_eq!(crate::fold_lam_alef(&[0x0644]), None);
    assert_eq!(crate::fold_lam_alef(&[0x0627, 0x0644]), None);
    assert_eq!(crate::lam_alef_pair('a'), None);
}
