//! Backend selection.

use crossbeam_channel::Sender;
use tracing::info;

use crate::enums::PopupUiEvent;
use crate::traits::SuggestionPopup;

#[cfg(target_os = "linux")]
use crate::linux::create_for_platform;
#[cfg(target_os = "macos")]
use crate::macos::create_for_platform;
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
use crate::noop::create_for_platform;
#[cfg(windows)]
use crate::windows::create_for_platform;

/// Create the tooltip backend for this platform. `events` receives
/// clicks and timeouts; the caller routes them to the engine.
pub fn create_popup(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    let popup = create_for_platform(events);
    info!(backend = popup.backend_name(), "suggestion popup backend");
    popup
}
