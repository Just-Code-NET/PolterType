//! Plain data: what the panel currently has on screen.

use crate::types::{PopupModel, RenderedPopup};

/// What is on screen right now.
pub(super) struct Shown {
    pub(super) model: PopupModel,
    pub(super) rendered: RenderedPopup,
    /// Device scale the frame was rendered at (Retina = 2.0).
    pub(super) scale: f64,
    pub(super) hover: Option<usize>,
}
