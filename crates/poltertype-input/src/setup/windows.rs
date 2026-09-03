//! What a Windows user has to grant — nothing.

use super::enums::{Permission, StepState};
use super::types::{SetupReport, SetupStep};

pub(super) fn probe(_local_signing_identity: &str) -> SetupReport {
    SetupReport {
        backend: Some("windows-ll-hook".to_owned()),
        steps: vec![SetupStep {
            title: "Nothing to set up on Windows".to_owned(),
            detail: "The low-level keyboard hook PolterType uses needs no permission and \
                     no elevation — it works from a normal user account the moment the \
                     app starts."
                .to_owned(),
            state: StepState::Done,
            action: None,
        }],
    }
}

/// No system dialog to trigger here — see [`probe`].
pub(super) fn request(_permission: Permission) -> bool {
    false
}

pub(super) fn settings_pane_url(_permission: Permission) -> Option<&'static str> {
    None
}

pub(super) fn setup_local_signing(_name: &str) -> Result<(), String> {
    Err("local update signing is a macOS mechanism".to_owned())
}
