//! Plain data the X11 state is built from: resolved EWMH atoms, the
//! chosen depth/visual, and the currently mapped window.

use std::time::Instant;

use x11rb::protocol::xproto::{Atom, Gcontext, Visualid, Window};

use crate::types::{PopupModel, RenderedPopup};

/// EWMH atoms, resolved once. Best-effort: a server without them just
/// skips the hints (override-redirect works regardless).
pub(super) struct Atoms {
    pub(super) window_type: Atom,
    pub(super) window_type_tooltip: Atom,
    pub(super) wm_state: Atom,
    pub(super) wm_state_above: Atom,
}

/// The depth/visual the popup window is created with. 32-bit TrueColor
/// when the server offers one (real transparency under a compositor);
/// otherwise the root visual with an opaque panel.
pub(super) struct VisualPick {
    pub(super) depth: u8,
    pub(super) visual: Visualid,
    pub(super) colormap: Option<u32>,
    pub(super) argb: bool,
}

/// The currently mapped window and what it shows.
pub(super) struct WinView {
    pub(super) window: Window,
    pub(super) gc: Gcontext,
    pub(super) rendered: RenderedPopup,
    pub(super) model: PopupModel,
    pub(super) hover: Option<usize>,
    pub(super) deadline: Instant,
}
