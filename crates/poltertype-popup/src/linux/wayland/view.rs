//! What [`super::state::WlState`] shows right now.

use std::time::Instant;

use smithay_client_toolkit::shell::wlr_layer::LayerSurface;

use crate::types::{PopupModel, RenderedPopup};

/// Everything shown right now. Dropped whole on hide — destroying the
/// `LayerSurface` (and its inner `wl_surface`) is the simplest correct
/// unmap.
pub(super) struct View {
    pub(super) layer: LayerSurface,
    pub(super) rendered: RenderedPopup,
    pub(super) model: PopupModel,
    pub(super) scale: i32,
    pub(super) hover: Option<usize>,
    /// No buffer may be attached before the first configure.
    pub(super) configured: bool,
    pub(super) deadline: Instant,
}
