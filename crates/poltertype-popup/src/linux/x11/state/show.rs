//! Creating and placing the window: [`X11State::show`] tears down any
//! current window and maps a fresh one; `place` is the shared
//! side-picker its placement is measured against.

use std::time::Instant;

use tracing::debug;
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, PropMode,
    StackMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use crate::enums::PopupAnchor;
use crate::types::PopupModel;

use super::types::WinView;
use super::x11_state::X11State;

/// Popup bottom edge floats this many px above the anchor window's
/// bottom edge (or the screen bottom).
const BOTTOM_OFFSET: i32 = 96;

impl X11State {
    /// Create, hint, map and paint a window for `model`.
    /// Destroy-and-recreate per show avoids resize handling and stale
    /// state.
    pub(super) fn show(&mut self, model: PopupModel) -> Result<(), ReplyOrIdError> {
        self.destroy_window();

        // X11 has no per-monitor scale story worth chasing here.
        let rendered = self.renderer.render(&model, None, 1.0);
        let w = rendered.pixmap.width().min(u16::MAX as u32) as u16;
        let h = rendered.pixmap.height().min(u16::MAX as u32) as u16;
        let (x, y) = self.place(w, h, &model.anchor);

        let window = self.conn.generate_id()?;
        // For a depth ≠ root windows, border_pixel and colormap are
        // mandatory or CreateWindow fails with BadMatch.
        let mut aux = CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(0)
            .border_pixel(0)
            .event_mask(
                EventMask::BUTTON_PRESS
                    | EventMask::POINTER_MOTION
                    | EventMask::LEAVE_WINDOW
                    | EventMask::EXPOSURE,
            );
        if let Some(colormap) = self.visual.colormap {
            aux = aux.colormap(colormap);
        }
        self.conn.create_window(
            self.visual.depth,
            window,
            self.root,
            x,
            y,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            self.visual.visual,
            &aux,
        )?;

        // Best-effort EWMH hints; harmless to skip if atoms missing.
        if let Some(atoms) = &self.atoms {
            let _ = self.conn.change_property32(
                PropMode::REPLACE,
                window,
                atoms.window_type,
                AtomEnum::ATOM,
                &[atoms.window_type_tooltip],
            );
            let _ = self.conn.change_property32(
                PropMode::REPLACE,
                window,
                atoms.wm_state,
                AtomEnum::ATOM,
                &[atoms.wm_state_above],
            );
        }

        let gc = self.conn.generate_id()?;
        self.conn.create_gc(gc, window, &CreateGCAux::new())?;
        self.conn.map_window(window)?;
        self.conn.configure_window(
            window,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;

        debug!(
            entries = model.entries.len(),
            w,
            h,
            argb = self.visual.argb,
            "popup window mapped"
        );
        let deadline = Instant::now() + model.timeout;
        self.win = Some(WinView {
            window,
            gc,
            rendered,
            model,
            hover: None,
            deadline,
        });
        self.paint()?;
        self.conn.flush()?;
        Ok(())
    }

    /// Root-coordinate placement: the shared side-picker around the
    /// pointer for `Point`, centred on the anchor window with the
    /// bottom edge `BOTTOM_OFFSET` above its bottom for `WindowRect`;
    /// clamped to the screen either way.
    fn place(&self, w: u16, h: u16, anchor: &PopupAnchor) -> (i16, i16) {
        let (px, py) = match *anchor {
            PopupAnchor::Point { x, y, height, .. } => crate::place::place_near_point(
                x,
                y,
                y + height as i32,
                w as i32,
                h as i32,
                Some((self.screen_w as i32, self.screen_h as i32)),
            ),
            PopupAnchor::WindowRect {
                x,
                y,
                width,
                height,
                ..
            } => (
                x + (width as i32 - w as i32) / 2,
                y + height as i32 - BOTTOM_OFFSET - h as i32,
            ),
            PopupAnchor::ScreenBottom => (
                (self.screen_w as i32 - w as i32) / 2,
                self.screen_h as i32 - BOTTOM_OFFSET - h as i32,
            ),
        };
        (
            px.clamp(0, (self.screen_w as i32 - w as i32).max(0)) as i16,
            py.clamp(0, (self.screen_h as i32 - h as i32).max(0)) as i16,
        )
    }
}
