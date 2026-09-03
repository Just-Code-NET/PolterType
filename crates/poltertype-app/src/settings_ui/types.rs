//! Plain data shared across the window's panes.

use poltertype_core::engine::ModSet;

#[derive(Debug, Clone)]
pub struct SaveBanner {
    pub(super) text: String,
    pub(super) is_error: bool,
}

/// Capture state for a modifier-only chord (issue #32), mirroring what
/// the engine's matcher does with the live key stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModCapture {
    /// Modifier keys down right now.
    pub(super) down: ModSet,
    /// Every modifier seen during this hold — the gesture is judged on
    /// what was held together, not on what is left at the last release.
    pub(super) peak: ModSet,
    /// A single-modifier tap that has landed and is waiting to see
    /// whether a second one follows. One modifier alone is never a
    /// binding, so nothing is committed until it does.
    pub(super) pending_tap: Option<ModSet>,
}
