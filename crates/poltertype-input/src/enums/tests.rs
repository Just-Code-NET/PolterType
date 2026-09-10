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
