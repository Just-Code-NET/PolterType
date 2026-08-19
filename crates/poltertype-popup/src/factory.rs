//! Backend selection.

use crossbeam_channel::Sender;
use tracing::info;

use crate::enums::PopupUiEvent;
// macOS is the one target whose single backend cannot fail, so the
// fallback stays out of that build entirely.
#[cfg(not(target_os = "macos"))]
use crate::noop::NoopPopup;
use crate::traits::SuggestionPopup;

/// Create the tooltip backend for this platform. `events` receives
/// clicks and timeouts; the caller routes them to the engine.
///
/// Linux selection mirrors the input-listener probe order, and it is a
/// *probe* rather than a lookup of desktop names — which matters,
/// because the names in the docs were wrong for two releases while the
/// probe was right. Wayland → layer-shell, then `DISPLAY` →
/// override-redirect (XWayland still produces a visible tooltip on
/// GNOME), then noop.
pub fn create_popup(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    let popup = create_for_platform(events);
    info!(backend = popup.backend_name(), "suggestion popup backend");
    popup
}

#[cfg(target_os = "linux")]
fn create_for_platform(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match crate::linux::wayland::WaylandPopup::try_new(events.clone()) {
            Ok(p) => return Box::new(p),
            Err(e) => {
                tracing::warn!(err = %e, "layer-shell popup unavailable; probing X11");
            }
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        match crate::linux::x11::X11Popup::try_new(events) {
            Ok(p) => return Box::new(p),
            Err(e) => {
                tracing::warn!(err = %e, "X11 popup unavailable");
            }
        }
    }
    Box::new(NoopPopup)
}

/// Windows needs nothing probed: a layered topmost window exists on
/// every version we ship to. Creation can still fail — a session with
/// no interactive window station, for one — and then the tooltip
/// degrades to the keyboard accept chord as it does elsewhere.
#[cfg(windows)]
fn create_for_platform(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    match crate::windows::WindowsPopup::try_new(events) {
        Ok(p) => Box::new(p),
        Err(e) => {
            tracing::warn!(err = %e, "layered popup unavailable");
            Box::new(NoopPopup)
        }
    }
}

/// macOS gets the `NSPanel` backend, which cannot fail at construction
/// — see `MacosPopup::new`.
#[cfg(target_os = "macos")]
fn create_for_platform(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    Box::new(crate::macos::MacosPopup::new(events))
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn create_for_platform(_events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    Box::new(NoopPopup)
}
