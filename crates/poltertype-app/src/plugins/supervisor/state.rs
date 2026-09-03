//! `Supervisor` — every plug-in process this app started.

use super::types::Running;

/// Owns every plug-in process this app started.
#[derive(Default)]
pub struct Supervisor {
    pub(super) running: Vec<Running>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }
}
