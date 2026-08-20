//! The advice, not the probes: every branch of the message must name a
//! different action, and only one of the five may mention the setup
//! script — that is the whole reason this module exists (issue #31).

use super::*;

fn facts(nodes: usize, opened: usize, keyboards: usize) -> ScanFacts {
    ScanFacts {
        nodes: Some(nodes),
        opened,
        keyboards,
        first_error: Some("Permission denied (os error 13)".to_owned()),
        sample: Some(NodeFacts {
            name: "/dev/input/event3".to_owned(),
            uid: 0,
            gid: 0,
            mode: 0o600,
        }),
    }
}

#[test]
fn unlistable_directory_does_not_blame_the_user() {
    let f = ScanFacts {
        nodes: None,
        first_error: Some("No such file or directory (os error 2)".to_owned()),
        ..ScanFacts::default()
    };
    let msg = no_keyboards_message(&f, GroupState::Absent);
    assert!(msg.contains("could not be listed"), "{msg}");
    assert!(!msg.contains("setup-linux.sh"), "{msg}");
}

#[test]
fn a_machine_with_no_input_devices_is_not_a_permission_problem() {
    let msg = no_keyboards_message(&facts(0, 0, 0), GroupState::Absent);
    assert!(msg.contains("nothing to read"), "{msg}");
    assert!(!msg.contains("setup-linux.sh"), "{msg}");
}

#[test]
fn devices_open_but_none_is_a_keyboard_points_at_remappers() {
    let msg = no_keyboards_message(&facts(12, 4, 0), GroupState::Active);
    assert!(msg.contains("none of them is a keyboard"), "{msg}");
    assert!(msg.contains("keyd"), "{msg}");
    assert!(!msg.contains("setup-linux.sh"), "{msg}");
}

#[test]
fn group_granted_but_session_predates_it_says_log_out() {
    let msg = no_keyboards_message(&facts(12, 0, 0), GroupState::InDatabaseOnly);
    assert!(msg.contains("log out and back in"), "{msg}");
    // The state the reporter of #31 was left guessing at: re-running the
    // script cannot help, and `newgrp` in a terminal does nothing for an
    // app started from the desktop.
    assert!(!msg.contains("scripts/setup-linux.sh"), "{msg}");
    assert!(msg.contains("newgrp"), "{msg}");
}

#[test]
fn no_membership_at_all_is_the_one_case_that_wants_the_script() {
    let msg = no_keyboards_message(&facts(12, 0, 0), GroupState::Absent);
    assert!(msg.contains("bash scripts/setup-linux.sh"), "{msg}");
}

#[test]
fn group_active_yet_nothing_opens_reports_what_the_kernel_says() {
    let msg = no_keyboards_message(&facts(12, 0, 0), GroupState::Active);
    assert!(
        msg.contains("/dev/input/event3 is uid=0 gid=0 mode=0600"),
        "{msg}"
    );
    assert!(msg.contains("Permission denied"), "{msg}");
    assert!(msg.contains("re-triggers udev"), "{msg}");
}

#[test]
fn a_missing_sample_leaves_the_sentence_intact() {
    let f = ScanFacts {
        sample: None,
        ..facts(12, 0, 0)
    };
    let msg = no_keyboards_message(&f, GroupState::Active);
    assert!(!msg.contains("()"), "{msg}");
    assert!(msg.contains("re-triggers udev"), "{msg}");
}
