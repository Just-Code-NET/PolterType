use super::*;

/// The name half of the check, kept pure: `session_is_wlroots` also
/// scans `/proc`, so asserting on it would pass or fail depending
/// on what the machine running the tests happens to be.
///
/// `XDG_CURRENT_DESKTOP` on the guest reads `labwc:wlroots`, so
/// either entry has to be enough — and entries are matched whole,
/// because `swaything` is a fork whose input stack nobody has seen.
#[test]
fn wlroots_sessions_are_recognised_by_either_half_of_the_name() {
    for value in [
        "labwc", "wlroots", "sway", "SWAY", "river", "niri", "wayfire",
    ] {
        assert!(is_wlroots_name(value), "{value} should read as wlroots");
    }
    for value in ["GNOME", "ubuntu", "KDE", "swaything", "Budgie", ""] {
        assert!(
            !is_wlroots_name(value),
            "{value} should not read as wlroots"
        );
    }
    // Display managers write whole paths into these variables.
    assert!(is_wlroots_name("/usr/share/wayland-sessions/sway"));
}

/// The list that decides whether this backend claims a session at
/// all. Kept pure for the same reason as the one above: reading the
/// environment would assert on whatever desktop runs the suite.
#[test]
fn only_the_desktops_that_act_on_the_schema_are_named() {
    let names = |value: &str| {
        value
            .split(':')
            .any(|e| names_any_of(e, &GNOME_FAMILY_NAMES))
    };

    // Ubuntu's GNOME announces itself with two entries; either is
    // enough.
    for value in [
        "GNOME",
        "ubuntu:GNOME",
        "gnome",
        "GNOME-Flashback:GNOME",
        "Unity",
        "Budgie:GNOME",
        "Pantheon",
    ] {
        assert!(names(value), "{value} should read as GNOME-family");
    }
    // The six that took this backend on a machine whose dconf had
    // been populated by an earlier GNOME session, and could not
    // switch a thing (2026-08-27), plus the two that had a branch
    // of their own before.
    for value in [
        "i3",
        "ICEWM",
        "LXQt",
        "XFCE",
        "",
        "X-Cinnamon",
        "MATE",
        "sway:wlroots",
    ] {
        assert!(!names(value), "{value} must not claim this backend");
    }
}
