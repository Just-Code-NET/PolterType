//! Decision policy stub: the live logic is in
//! [`crate::engine::SwitcherEngine::decide`]. Reserved for a richer
//! policy (per-app overrides, voting, hysteresis) once the UI has
//! enough knobs to justify the abstraction.

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
