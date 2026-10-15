//! Input enums: the replay-speed setting, and the keys the suggestion
//! tooltip can claim.

use std::time::Duration;

use crate::ReplaySpeed;

#[test]
fn every_speed_survives_a_round_trip_through_the_config() {
    for speed in [ReplaySpeed::Normal, ReplaySpeed::Fast, ReplaySpeed::Instant] {
        assert_eq!(ReplaySpeed::from_config(speed.config_value()), speed);
    }
}

#[test]
fn a_config_value_nobody_recognises_reads_as_the_measured_pacing() {
    // A hand-edited file is the normal way this setting is written, so
    // a typo must land on the setting that cannot lose keystrokes.
    for garbage in ["", "quick", "FASTEST", "1", " "] {
        assert_eq!(ReplaySpeed::from_config(garbage), ReplaySpeed::Normal);
    }
    assert_eq!(ReplaySpeed::from_config(" Instant "), ReplaySpeed::Instant);
    assert_eq!(ReplaySpeed::default(), ReplaySpeed::Normal);
}

#[test]
fn each_step_up_costs_the_replay_less_of_its_pacing() {
    let step = Duration::from_millis(4);
    assert_eq!(ReplaySpeed::Normal.pace(step), step);
    assert_eq!(ReplaySpeed::Fast.pace(step), Duration::from_millis(2));
    assert_eq!(ReplaySpeed::Instant.pace(step), Duration::ZERO);
}

/// A pace may vanish; a settle may not.
///
/// The two wait on different things — see `ReplaySpeed::settle`. A
/// zero-length settle would replay into the layout we just left, which
/// is why the fastest gear keeps a quarter rather than nothing.
#[test]
fn the_fastest_gear_still_waits_for_another_process() {
    let window = Duration::from_millis(40);
    assert_eq!(ReplaySpeed::Normal.settle(window), window);
    assert_eq!(ReplaySpeed::Fast.settle(window), Duration::from_millis(20));
    assert_eq!(
        ReplaySpeed::Instant.settle(window),
        Duration::from_millis(10)
    );
    for gear in [ReplaySpeed::Normal, ReplaySpeed::Fast, ReplaySpeed::Instant] {
        assert!(
            gear.settle(window) > Duration::ZERO,
            "{gear:?} scaled a settle to nothing"
        );
    }
}

/// The byte the atomic in a running emitter holds is the setting, and
/// an unknown one is the safe default rather than a panic.
#[test]
fn a_gear_survives_the_trip_through_one_byte() {
    for gear in [ReplaySpeed::Normal, ReplaySpeed::Fast, ReplaySpeed::Instant] {
        assert_eq!(ReplaySpeed::from_u8(gear.as_u8()), gear);
    }
    assert_eq!(ReplaySpeed::from_u8(7), ReplaySpeed::Normal);
}

/// Each claimable key reads back from both of the engine's code spaces,
/// and no two of those codes name different keys.
#[test]
fn claim_keys_read_back_from_sc1_and_evdev_codes() {
    use crate::ClaimKey;
    for (sc, key) in [
        (0x48, ClaimKey::Up),
        (0x67, ClaimKey::Up),
        (0x50, ClaimKey::Down),
        (0x6C, ClaimKey::Down),
        (0x1C, ClaimKey::Enter),
        (0x60, ClaimKey::Enter),
        (0x3B, ClaimKey::Function(1)),
        (0x43, ClaimKey::Function(9)),
    ] {
        assert_eq!(ClaimKey::from_scancode(sc), Some(key), "scancode {sc:#x}");
    }
    assert_eq!(
        ClaimKey::from_scancode(0x44),
        None,
        "F10 is not a suggestion key"
    );
    assert_eq!(ClaimKey::from_scancode(0x02), Some(ClaimKey::Digit(1)));
    assert_eq!(ClaimKey::from_scancode(0x0A), Some(ClaimKey::Digit(9)));
    assert_eq!(ClaimKey::from_scancode(0x0B), None, "0 picks nothing");
}

#[test]
fn a_claim_set_holds_exactly_what_was_put_in() {
    use crate::{ClaimKey, ClaimSet};
    let set = ClaimSet::default()
        .with(ClaimKey::Down)
        .with(ClaimKey::Function(3));
    assert!(set.contains(ClaimKey::Down));
    assert!(set.contains(ClaimKey::Function(3)));
    assert!(!set.contains(ClaimKey::Up));
    assert!(!set.contains(ClaimKey::Function(4)));
    assert_eq!(
        set.keys().collect::<Vec<_>>(),
        vec![ClaimKey::Down, ClaimKey::Function(3)]
    );
    assert!(ClaimSet::default().is_empty());
    let chord = crate::Modifiers {
        control: true,
        shift: true,
        ..crate::Modifiers::NONE
    };
    let digits = ClaimSet::default().with_digit_modifiers(chord);
    assert!(digits.is_empty(), "modifiers alone claim nothing");
    let digits = digits.with(ClaimKey::Digit(2));
    assert_eq!(digits.digit_modifiers(), chord);
    assert_eq!(digits.keys().collect::<Vec<_>>(), vec![ClaimKey::Digit(2)]);
}
