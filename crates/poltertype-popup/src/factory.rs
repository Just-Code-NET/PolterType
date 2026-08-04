//! Backend selection.

use crossbeam_channel::Sender;
use tracing::info;

use crate::enums::PopupUiEvent;
use crate::noop::NoopPopup;
use crate::traits::SuggestionPopup;

/// Create the tooltip backend for this platform. `events` receives
/// clicks and timeouts; the caller routes them to the engine.
///
/// Selection on Linux mirrors the input-listener probe order, and it
/// is a *probe*, not a lookup of desktop names — which matters,
/// because the names in the docs were wrong for two releases while the
/// probe was right. Wayland session → layer-shell (wlroots
/// compositors and KWin; Mutter has none, detected at connect time).
/// Then `DISPLAY` → override-redirect window, which on a GNOME Wayland
/// session means XWayland and still produces a visible tooltip.
/// Nothing left → noop.
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

/// Windows has one backend and it needs nothing probed: a layered
/// topmost window is available on every version we ship to. It can
/// still fail to be created (a session with no interactive window
/// station, for one), and then the tooltip degrades to the keyboard
/// accept chord exactly as it does elsewhere.
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

#[cfg(not(any(target_os = "linux", windows)))]
fn create_for_platform(_events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    Box::new(NoopPopup)
}
