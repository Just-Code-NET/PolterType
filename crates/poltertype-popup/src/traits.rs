//! The popup extension point.

use crate::types::PopupModel;

/// A tooltip backend. Both methods are fire-and-forget: they enqueue
/// to the popup's own thread and return immediately (the engine's
/// hot path must never wait on window-system I/O).
pub trait SuggestionPopup: Send + Sync {
    /// Show `model`, replacing whatever is currently displayed.
    fn show(&self, model: PopupModel);

    /// Hide immediately (offer dismissed / applied). Idempotent.
    fn hide(&self);

    fn backend_name(&self) -> &'static str;
}
