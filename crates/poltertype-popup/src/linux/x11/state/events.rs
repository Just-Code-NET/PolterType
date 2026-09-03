//! The X event loop: draining events, hover repaint, deadline timeout
//! and window teardown.

use std::time::Instant;

use tracing::warn;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::ConnectionExt;

use crate::enums::PopupUiEvent;
use crate::render::hit_row;

use super::x11_state::X11State;

impl X11State {
    /// Drain the X event queue. Returns `false` when the connection is
    /// gone and the thread should exit.
    pub(super) fn pump_events(&mut self) -> bool {
        loop {
            let event = match self.conn.poll_for_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(e) => {
                    warn!(err = %e, "x11 popup thread lost its connection");
                    return false;
                }
            };
            self.handle_event(event);
        }
        let _ = self.conn.flush();
        true
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Expose(e) => {
                if self.win.as_ref().is_some_and(|v| v.window == e.window) && e.count == 0 {
                    if let Err(err) = self.paint() {
                        warn!(err = %err, "x11 popup repaint failed");
                    }
                }
            }
            Event::MotionNotify(e) => {
                if self.win.as_ref().is_some_and(|v| v.window == e.event) {
                    let hover = self.win.as_ref().and_then(|v| {
                        hit_row(&v.rendered.rows, e.event_x as f32, e.event_y as f32)
                    });
                    self.set_hover(hover);
                }
            }
            Event::LeaveNotify(e) => {
                if self.win.as_ref().is_some_and(|v| v.window == e.event) {
                    self.set_hover(None);
                }
            }
            Event::ButtonPress(e) => {
                let hit = self.win.as_ref().and_then(|v| {
                    (v.window == e.event && e.detail == 1)
                        .then(|| hit_row(&v.rendered.rows, e.event_x as f32, e.event_y as f32))
                        .flatten()
                        .map(|index| (v.model.generation, index))
                });
                if let Some((generation, index)) = hit {
                    // Hide first so the popup vanishes the instant the
                    // engine starts retyping.
                    self.destroy_window();
                    let _ = self
                        .events
                        .send(PopupUiEvent::Accepted { generation, index });
                }
            }
            _ => {}
        }
    }

    /// Re-render on hover change and repaint in place.
    fn set_hover(&mut self, hover: Option<usize>) {
        let Some(view) = &mut self.win else { return };
        if view.hover == hover {
            return;
        }
        view.hover = hover;
        view.rendered = self.renderer.render(&view.model, hover, 1.0);
        if let Err(err) = self.paint() {
            warn!(err = %err, "x11 popup repaint failed");
        }
    }

    pub(super) fn check_deadline(&mut self) {
        let Some(view) = &self.win else { return };
        if Instant::now() < view.deadline {
            return;
        }
        let generation = view.model.generation;
        self.destroy_window();
        let _ = self.events.send(PopupUiEvent::TimedOut { generation });
    }

    pub(super) fn destroy_window(&mut self) {
        if let Some(view) = self.win.take() {
            let _ = self.conn.free_gc(view.gc);
            let _ = self.conn.destroy_window(view.window);
            let _ = self.conn.flush();
        }
    }
}
