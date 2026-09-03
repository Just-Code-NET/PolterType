//! The setup probe on a target with no known backend.

use super::enums::Permission;
use super::types::SetupReport;

pub(super) fn probe(_local_signing_identity: &str) -> SetupReport {
    SetupReport {
        backend: None,
        steps: Vec::new(),
    }
}

pub(super) fn request(_permission: Permission) -> bool {
    false
}

pub(super) fn settings_pane_url(_permission: Permission) -> Option<&'static str> {
    None
}

pub(super) fn setup_local_signing(_name: &str) -> Result<(), String> {
    Err("local update signing is a macOS mechanism".to_owned())
}
