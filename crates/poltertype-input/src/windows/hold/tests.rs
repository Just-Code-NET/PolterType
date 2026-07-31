//! Tests for the swallow decision.
//!
//! These run on every host, which is the point: the machine this
//! project is developed on has no Windows, and the property under test
//! — "the user's keyboard always comes back" — is the one nobody wants
//! to discover is false on someone else's computer.

use super::*;

#[test]
fn a_gate_that_was_never_asked_swallows_nothing() {
    let g = HoldState::new();
    assert!(!g.swallow(false, g.now_ms()));
    assert!(!g.is_holding());
}

#[test]
fn holding_swallows_the_users_keys() {
    let g = HoldState::new();
    g.hold();
    assert!(g.swallow(false, g.now_ms()));
}

/// The self-deadlock this check exists to prevent: swallowing our own
/// correction means the backspaces and the retyped word never reach the
/// application, i.e. the feature silently does nothing.
#[test]
fn our_own_keystrokes_are_never_swallowed() {
    let g = HoldState::new();
    g.hold();
    assert!(!g.swallow(true, g.now_ms()));
}

#[test]
fn releasing_lets_keys_through_again() {
    let g = HoldState::new();
    g.hold();
    g.release();
    assert!(!g.swallow(false, g.now_ms()));
}

/// The load-bearing test of this module. A caller that asks for a hold
/// and never releases it — panicked, deadlocked, or simply buggy —
/// must not leave the keyboard dead.
#[test]
fn a_hold_nobody_releases_expires_by_itself() {
    let g = HoldState::new();
    g.hold();
    let past_deadline = g.now_ms() + MAX_HOLD.as_millis() as u64 + 1;
    assert!(
        !g.swallow(false, past_deadline),
        "an expired hold must not swallow"
    );
}

/// And it must not merely decline once: the expired hold has to be
/// *cleared*, or every later keystroke pays the deadline comparison and
/// a clock that jumps backwards would resurrect it.
#[test]
fn an_expired_hold_clears_itself_rather_than_declining_each_time() {
    let g = HoldState::new();
    g.hold();
    let past_deadline = g.now_ms() + MAX_HOLD.as_millis() as u64 + 1;
    let _ = g.swallow(false, past_deadline);
    assert!(
        !g.is_holding(),
        "expiry must clear the request, not mask it"
    );
    // Even asked again with a time *before* the old deadline, it stays
    // released — the state is gone, not merely out of date.
    assert!(!g.swallow(false, 0));
}

#[test]
fn the_deadline_is_exclusive_at_its_own_millisecond() {
    let g = HoldState::new();
    g.hold_until(1000);
    assert!(g.swallow(false, 999), "still inside the window");
    assert!(!g.swallow(false, 1000), "the deadline itself is expiry");
}

/// A second correction starting while the first is still in flight must
/// get its own full window, not the remainder of the previous one.
#[test]
fn a_fresh_hold_extends_the_deadline() {
    let g = HoldState::new();
    g.hold_until(100);
    g.hold_until(5_000);
    assert!(g.swallow(false, 4_999));
}

/// `release` is called from `Drop`, which can run more than once on
/// paths that release early and then unwind.
#[test]
fn release_is_idempotent() {
    let g = HoldState::new();
    g.hold();
    g.release();
    g.release();
    assert!(!g.swallow(false, g.now_ms()));
}

/// Our own events pass whether or not a hold is in force — the check
/// must not depend on gate state, or a correction would deadlock
/// exactly when the gate is doing its job.
#[test]
fn ours_passes_in_every_state() {
    let g = HoldState::new();
    assert!(!g.swallow(true, g.now_ms()));
    g.hold();
    assert!(!g.swallow(true, g.now_ms()));
    g.release();
    assert!(!g.swallow(true, g.now_ms()));
}
