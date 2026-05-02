//! Decision policy stub.
//!
//! The bulk of the decision logic currently lives in
//! [`crate::engine::SwitcherEngine::decide`], which runs detectors in
//! priority order and picks the first verdict that clears the
//! engine's confidence threshold. This module exists as the future
//! home for a richer policy (per-app overrides, multi-detector
//! voting, hysteresis to avoid flip-flopping). Phase 4+ will move
//! the logic here once the UI surfaces enough knobs to justify the
//! abstraction.

#[derive(Debug, Clone, Copy)]
pub struct DecisionPolicy {
    pub min_confidence: f32,
}

impl Default for DecisionPolicy {
    fn default() -> Self {
        Self {
            min_confidence: 0.55,
        }
    }
}
