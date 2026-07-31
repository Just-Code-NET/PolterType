//! What the setup walkthrough shows the user.

use super::enums::{StepAction, StepState};

/// One thing the user may have to do, and whether they have done it.
///
/// Deliberately data, not widgets: the probe lives in this crate
/// because that is where the platform code is allowed to live, and the
/// Settings window renders whatever it is handed. That also makes the
/// per-OS logic testable without a GUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStep {
    /// Short imperative label — "Grant Accessibility", "Join the
    /// `input` group".
    pub title: String,
    /// One or two sentences saying what this is for and what the user
    /// will see. Written for someone who has never heard of evdev.
    pub detail: String,
    pub state: StepState,
    /// The one thing the button on this row does, if there is one.
    pub action: Option<StepAction>,
}

/// The whole picture, re-probed every time the user asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReport {
    /// Which listener backend this session would use, for the log-ish
    /// line at the bottom of the pane. `None` when no backend applies.
    pub backend: Option<String>,
    pub steps: Vec<SetupStep>,
}

impl SetupReport {
    /// True when at least one step is not satisfied — what the tray
    /// alert and the pane's headline key off.
    pub fn needs_attention(&self) -> bool {
        self.steps.iter().any(|s| s.state != StepState::Done)
    }
}
