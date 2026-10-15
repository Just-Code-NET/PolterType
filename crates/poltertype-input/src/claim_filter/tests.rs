//! Tests for the hooks' per-keystroke claim decision. They run on every
//! host — the hooks themselves only on Windows and macOS CI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::{ClaimSet, Modifiers};

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

fn armed(keys: ClaimSet) -> ClaimFilter {
    let claims = Arc::new(KeyClaims::default());
    claims.arm(keys, Instant::now() + Duration::from_secs(30));
    ClaimFilter::new(claims)
}

fn enter() -> ClaimSet {
    ClaimSet::default().with(ClaimKey::Enter)
}

const PRESS: bool = true;
const RELEASE: bool = false;
const BARE: Modifiers = Modifiers::NONE;
const USER: bool = false;

#[test]
fn an_unclaimed_key_passes() {
    let f = armed(enter());
    assert!(!f.decide(Some(ClaimKey::Down), PRESS, BARE, USER));
    assert!(!f.decide(None, PRESS, BARE, USER));
}

#[test]
fn a_claimed_key_is_kept_back_press_and_release() {
    let f = armed(enter());
    assert!(f.decide(Some(ClaimKey::Enter), PRESS, BARE, USER));
    assert!(f.decide(Some(ClaimKey::Enter), RELEASE, BARE, USER));
    // The release closed it: an unclaimed release after that passes.
    assert!(!f.decide(Some(ClaimKey::Enter), RELEASE, BARE, USER));
}

/// `Shift+Down` extends a selection; `Ctrl+Enter` sends a message.
#[test]
fn a_claimed_key_with_a_modifier_held_passes() {
    let f = armed(enter());
    assert!(!f.decide(Some(ClaimKey::Enter), PRESS, SHIFT, USER));
}

#[test]
fn our_own_keystrokes_are_never_claimed() {
    let f = armed(enter());
    assert!(!f.decide(Some(ClaimKey::Enter), PRESS, BARE, true));
}

/// The engine drops its claim the moment Enter accepts, while the key
/// is still down. Its repeats and its release must not reach the
/// application on their own — a lone autorepeated Enter is a newline.
#[test]
fn a_key_kept_back_stays_kept_back_until_released_even_after_the_claim_ends() {
    let claims = Arc::new(KeyClaims::default());
    claims.arm(enter(), Instant::now() + Duration::from_secs(30));
    let f = ClaimFilter::new(Arc::clone(&claims));
    assert!(f.decide(Some(ClaimKey::Enter), PRESS, BARE, USER));
    claims.disarm();
    assert!(
        f.decide(Some(ClaimKey::Enter), PRESS, BARE, USER),
        "autorepeat"
    );
    assert!(f.decide(Some(ClaimKey::Enter), RELEASE, BARE, USER));
    assert!(!f.decide(Some(ClaimKey::Enter), PRESS, BARE, USER));
}

/// A release that never comes must not leave the key dead.
#[test]
fn a_press_whose_release_never_came_stops_vouching_for_the_key() {
    let claims = Arc::new(KeyClaims::default());
    claims.arm(enter(), Instant::now() + Duration::from_secs(30));
    let f = ClaimFilter::new(Arc::clone(&claims));
    assert!(f.decide_at(Some(ClaimKey::Enter), PRESS, BARE, USER, 0));
    claims.disarm();
    let later = DOWN_STALE_MS + 1;
    assert!(!f.decide_at(Some(ClaimKey::Enter), PRESS, BARE, USER, later));
}

#[test]
fn an_expired_claim_keeps_nothing_back() {
    let claims = Arc::new(KeyClaims::default());
    claims.arm(enter(), Instant::now());
    let f = ClaimFilter::new(claims);
    assert!(!f.decide(Some(ClaimKey::Enter), PRESS, BARE, USER));
}

/// What the hooks call: the modifiers and direction come off the event.
#[test]
fn a_translated_event_is_judged_by_its_own_modifiers() {
    let f = armed(ClaimSet::default().with(ClaimKey::Down));
    let ev = |modifiers| KeyEvent {
        vk: 0,
        scancode: 0x50,
        direction: KeyDirection::Press,
        modifiers,
        injected: false,
        kernel_repeat: false,
        claimed: false,
        timestamp_ms: 0,
    };
    let shift = crate::Modifiers {
        shift: true,
        ..crate::Modifiers::NONE
    };
    assert!(
        !f.decide_event(Some(ClaimKey::Down), &ev(shift)),
        "Shift+Down selects"
    );
    assert!(f.decide_event(Some(ClaimKey::Down), &ev(crate::Modifiers::NONE)));
}

/// The accept chord's digit is claimed with its own modifiers and with
/// nothing else: a bare `1` is typing, and `Ctrl+1` some other shortcut.
#[test]
fn a_digit_is_claimed_only_with_the_chord_modifiers() {
    let chord = Modifiers {
        control: true,
        shift: true,
        ..Modifiers::NONE
    };
    let f = armed(
        ClaimSet::default()
            .with(ClaimKey::Digit(1))
            .with_digit_modifiers(chord),
    );
    assert!(!f.decide(Some(ClaimKey::Digit(1)), PRESS, BARE, USER));
    assert!(!f.decide(Some(ClaimKey::Digit(1)), PRESS, SHIFT, USER));
    assert!(f.decide(Some(ClaimKey::Digit(1)), PRESS, chord, USER));
    assert!(f.decide(Some(ClaimKey::Digit(1)), RELEASE, chord, USER));
    assert!(!f.decide(Some(ClaimKey::Digit(2)), PRESS, chord, USER));
}
