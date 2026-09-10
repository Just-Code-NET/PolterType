//! The replay-speed setting: parsing and what it scales.

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
