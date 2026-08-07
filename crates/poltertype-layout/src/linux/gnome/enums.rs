//! What the probe does when something else mediates input.

/// Whether `try_init` is allowed to disqualify the schema because an
/// input-method daemon is between it and the keyboard.
pub(crate) enum Mediation {
    /// Probing: let the IBus backend have the session instead.
    StandDown,
    /// Pinned by the user through `POLTERTYPE_LAYOUT_BACKEND`: they
    /// have seen this work on their machine, our name-based guess has
    /// not.
    Ignore,
}
