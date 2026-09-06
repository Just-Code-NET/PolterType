//! What the setup walkthrough shows the user.

use super::enums::{StepAction, StepState};

/// A sentence the Setup pane shows, and the catalog key that
/// translates it.
///
/// The words are written in this crate, where the platform knowledge
/// is, but the lookup cannot happen here: `tr` lives in
/// `poltertype-core`, which depends on this crate. So a step carries a
/// stable key and the English it falls back to, and the Settings
/// window translates as it draws — the same "a key and the English at
/// the call site" contract the rest of the interface has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupText {
    /// Catalog key, `setup.`-prefixed like the rest of the pane's.
    pub key: &'static str,
    /// The English original, compiled in, and what is shown whenever
    /// no catalog translates the key.
    pub english: &'static str,
    /// Values for the `{}` placeholders in `english`, in order. Empty
    /// for a sentence that has none, which is most of them.
    pub args: Vec<String>,
}

impl SetupText {
    pub fn new(key: &'static str, english: &'static str) -> Self {
        Self {
            key,
            english,
            args: Vec::new(),
        }
    }

    /// A sentence quoting something read off this machine. The value
    /// stays an argument rather than being formatted in here, so a
    /// translation can put it where its own grammar needs it.
    pub fn with_args(key: &'static str, english: &'static str, args: Vec<String>) -> Self {
        Self { key, english, args }
    }
}

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
    pub title: SetupText,
    /// One or two sentences saying what this is for and what the user
    /// will see. Written for someone who has never heard of evdev.
    pub detail: SetupText,
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
