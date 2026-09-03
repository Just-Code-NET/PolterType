//! Main-thread entry points [`super::popup`] dispatches onto, and the
//! storage — a thread-local `STATE` plus the events channel — every
//! other file in this module reads through.

use std::cell::RefCell;

use crossbeam_channel::Sender;
use objc2_foundation::MainThreadMarker;
use tracing::warn;

use crate::enums::PopupUiEvent;
use crate::types::PopupModel;

use super::consts::EVENTS;
use super::state::PanelState;

thread_local! {
    pub(super) static STATE: RefCell<Option<PanelState>> = const { RefCell::new(None) };
}

pub(super) fn register_events(events: Sender<PopupUiEvent>) {
    // A second registration means a second popup handle — the tests do
    // that; the first sender is as good as any.
    let _ = EVENTS.set(events);
}

/// Entry point for `show`. Creates the panel lazily on first use —
/// `create_popup` runs before the tao event loop starts, so this (and
/// everything else here) must survive being the first AppKit call the
/// process makes on the main queue.
pub(super) fn show_on_main(model: PopupModel) {
    let Some(mtm) = MainThreadMarker::new() else {
        warn!("suggestion popup: not on the main thread; dropping show");
        return;
    };
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = PanelState::create(mtm);
            if slot.is_none() {
                warn!("could not create the overlay panel; suggestions will not be shown");
                return;
            }
        }
        if let Some(state) = slot.as_mut() {
            state.show(model, mtm);
        }
    });
}

/// Entry point for `hide`. Idempotent.
pub(super) fn hide_on_main() {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.hide();
        }
    });
}
