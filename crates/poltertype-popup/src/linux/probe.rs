//! Backend selection for Linux.

use crossbeam_channel::Sender;
use tracing::warn;

use super::wayland::WaylandPopup;
use super::x11::X11Popup;
use crate::enums::PopupUiEvent;
use crate::noop::NoopPopup;
use crate::traits::SuggestionPopup;

/// A *probe* rather than a lookup of desktop names — which matters,
/// because the names in the docs were wrong for two releases while the
/// probe was right. Wayland → layer-shell, then `DISPLAY` →
/// override-redirect (XWayland still produces a visible tooltip on
/// GNOME), then noop.
pub(crate) fn create_for_platform(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match WaylandPopup::try_new(events.clone()) {
            Ok(p) => return Box::new(p),
            Err(e) => {
                warn!(err = %e, "layer-shell popup unavailable; probing X11");
            }
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        match X11Popup::try_new(events) {
            Ok(p) => return Box::new(p),
            Err(e) => {
                warn!(err = %e, "X11 popup unavailable");
            }
        }
    }
    Box::new(NoopPopup)
}
