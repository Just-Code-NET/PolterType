//! Reacting to AppKit and the self-hide timer: everything here runs on
//! [`super::dispatch::STATE`] from outside the synchronous
//! `show`/`hide` calls [`super::popup`] makes.

use objc2_foundation::NSPoint;

use crate::enums::PopupUiEvent;
use crate::render::hit_row;

use super::consts::EVENTS;
use super::dispatch::STATE;

/// The self-hide timer firing. Stale timers (a newer offer replaced
/// the one that scheduled them) match no generation and do nothing.
pub(super) fn timeout_fired(generation: u64) {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        let Some(shown) = &state.shown else { return };
        if shown.model.generation != generation {
            return;
        }
        state.hide();
        if let Some(events) = EVENTS.get() {
            let _ = events.send(PopupUiEvent::TimedOut { generation });
        }
    });
}

/// A click on the panel. Accepts the row under the pointer, if any;
/// a click on panel padding is ignored (same as the other backends).
pub(super) fn click_at(point: NSPoint) {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        let Some(shown) = &state.shown else { return };
        let s = shown.scale;
        let Some(index) = hit_row(
            &shown.rendered.rows,
            (point.x * s) as f32,
            (point.y * s) as f32,
        ) else {
            return;
        };
        let generation = shown.model.generation;
        state.hide();
        if let Some(events) = EVENTS.get() {
            let _ = events.send(PopupUiEvent::Accepted { generation, index });
        }
    });
}

pub(super) fn hover_at(point: Option<NSPoint>) {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        let hover = point.and_then(|p| {
            let shown = state.shown.as_ref()?;
            hit_row(
                &shown.rendered.rows,
                (p.x * shown.scale) as f32,
                (p.y * shown.scale) as f32,
            )
        });
        state.redraw_hover(hover);
    });
}
