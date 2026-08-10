//! macOS backend: a borderless, non-activating `NSPanel`.
//!
//! Unlike the other backends there is no popup thread — AppKit window
//! objects belong to the main thread, which the tao event loop owns —
//! so the handle below is zero-sized and every command hops onto the
//! main dispatch queue. The fire-and-forget contract holds by
//! construction: `exec_async` enqueues and returns, and the engine's
//! hot path never touches AppKit. See `docs/MACOS_POPUP.md`.

use crossbeam_channel::Sender;
use dispatch2::DispatchQueue;

use super::panel;
use crate::enums::PopupUiEvent;
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// Dispatching handle; the panel and all state live on the main
/// thread inside [`panel`].
pub struct MacosPopup;

impl MacosPopup {
    /// Cannot fail: the panel itself is created lazily on first
    /// `show`, once the event loop is actually pumping the main queue
    /// (`create_popup` runs before `event_loop.run`, so creating it
    /// here would deadlock a synchronous hop and race an async one).
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
