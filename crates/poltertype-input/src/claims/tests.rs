//! Tests for the claim state the engine arms and the listeners read.

use std::time::{Duration, Instant};

use super::*;
use crate::ClaimKey;

fn arrows() -> ClaimSet {
    ClaimSet::default().with(ClaimKey::Up).with(ClaimKey::Down)
}

#[test]
fn nothing_is_claimed_until_armed() {
    let c = KeyClaims::default();
    assert!(c.armed().is_empty());
    assert!(!c.supported());
}

#[test]
fn an_armed_claim_reads_back_until_its_deadline() {
    let c = KeyClaims::default();
    c.arm(arrows(), Instant::now() + Duration::from_secs(30));
    assert_eq!(c.armed(), arrows());
    c.disarm();
    assert!(c.armed().is_empty());
}

/// The property that keeps a wedged engine from eating the arrows for
/// good: nobody has to disarm an expired claim.
#[test]
fn a_claim_past_its_deadline_is_void() {
    let c = KeyClaims::default();
    c.arm(arrows(), Instant::now());
    assert!(c.armed().is_empty());
}

#[test]
fn a_listener_going_away_drops_the_claim() {
    let c = KeyClaims::default();
    c.set_supported(true);
    c.arm(arrows(), Instant::now() + Duration::from_secs(30));
    c.set_supported(false);
    assert!(!c.supported());
    assert!(c.armed().is_empty());
}
