//! Decision policy stub.
//!
//! The decision logic lives in
//! [`crate::engine::SwitcherEngine::decide`]. This module is the
//! reserved home for a richer policy — per-app overrides,
//! multi-detector voting, hysteresis — once the UI surfaces enough
//! knobs to justify the abstraction.

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
