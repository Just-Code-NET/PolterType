//! The public handle for the macOS backend — see [`super`] for the
//! threading model it implies. Zero-sized: `exec_async` enqueues and
//! returns, so the fire-and-forget contract holds by construction.

use crossbeam_channel::Sender;
use dispatch2::DispatchQueue;

use super::panel;
use crate::enums::PopupUiEvent;
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// macOS gets the `NSPanel` backend, which cannot fail at construction
/// — see [`MacosPopup::new`].
pub(crate) fn create_for_platform(events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    Box::new(MacosPopup::new(events))
}

/// Dispatching handle; the panel and all state live on the main
/// thread inside [`panel`].
pub struct MacosPopup;

impl MacosPopup {
    /// Cannot fail: the panel is created lazily on first `show`, once
    /// the event loop is pumping the main queue. `create_popup` runs
    /// before `event_loop.run`, so creating it here would deadlock a
    /// synchronous hop and race an async one.
    pub fn new(events: Sender<PopupUiEvent>) -> Self {
        panel::register_events(events);
        Self
    }
}

impl SuggestionPopup for MacosPopup {
    fn show(&self, model: PopupModel) {
        DispatchQueue::main().exec_async(move || panel::show_on_main(model));
    }

    fn hide(&self) {
        DispatchQueue::main().exec_async(panel::hide_on_main);
    }

    fn backend_name(&self) -> &'static str {
        "macos-nspanel-nonactivating"
    }
}
