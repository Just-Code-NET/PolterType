//! Tests for the setup probe.
//!
//! Deliberately about *shape*, not about this machine: a CI runner, a
//! developer laptop with the group already granted, and a fresh Wayland
//! install all give different answers, and any assertion about which
//! one is right would be an assertion about the test host. What must
//! hold everywhere is that the report is renderable and internally
//! consistent — the pane trusts both.

use super::*;

#[test]
fn the_probe_always_produces_something_renderable() {
    let report = probe_setup("");
    for step in &report.steps {
        assert!(!step.title.is_empty(), "a step with no title renders blank");
        assert!(
            !step.detail.is_empty(),
            "step `{}` has no explanation — the whole point of the pane",
            step.title
        );
    }
}

/// A step the user can do nothing about must not be presented as work.
/// The converse — a `Done` step with an action — is fine: macOS keeps
/// its "open the pane" button either way.
#[test]
fn every_actionable_step_says_what_to_do() {
    for step in probe_setup("").steps {
        if matches!(
            step.state,
            StepState::Todo | StepState::NeedsRelogin | StepState::NeedsReset
        ) {
            assert!(
                step.action.is_some(),
                "step `{}` tells the user they must act and gives them no way to",
                step.title
            );
        }
    }
}

#[test]
fn needs_attention_tracks_the_steps() {
    let report = probe_setup("");
    let unresolved = report
        .steps
        .iter()
        .filter(|s| s.state != StepState::Done)
        .count();
    assert_eq!(report.needs_attention(), unresolved > 0);
}

/// The empty report is the one case where "nothing to show" and
/// "nothing wrong" must agree — an unsupported platform has no steps
/// and must not light the tray up with a warning.
#[test]
fn a_platform_with_no_steps_needs_no_attention() {
    let empty = SetupReport {
        backend: None,
        steps: Vec::new(),
    };
    assert!(!empty.needs_attention());
}

#[test]
fn requesting_a_permission_is_a_noop_where_there_is_no_dialog() {
    // On macOS this would show a system dialog, so it is not called
    // here; everywhere else the contract is "returns false, does
    // nothing", which the Setup pane relies on to keep one code path.
    #[cfg(not(target_os = "macos"))]
    {
        assert!(!request_permission(Permission::Accessibility));
        assert!(permission_settings_url(Permission::InputMonitoring).is_none());
    }
}
