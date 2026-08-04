//! Windows backend: a layered, always-on-top, never-activated overlay.
//!
//! One dedicated thread owns the window and the renderer; the public
//! handle only pushes commands into a channel, the same shape as the
//! two Linux backends. The thread parks on a 16 ms tick, which is what
//! bounds how quickly a click on a row is noticed — nothing here is
//! animated.
//!
//! ## Where the mouse is handled, and why not in the window procedure
//!
//! Mouse messages are *posted*, so they arrive through the thread's
//! queue and can be read with `PeekMessageW` before being dispatched.
//! Doing the hit-test there — in this file, with the row rectangles in
//! scope — means the window procedure stays a bare `DefWindowProcW`
//! and none of this state has to be smuggled into a C callback through
//! `GWLP_USERDATA` and back out of a raw pointer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use thiserror::Error;
use tracing::{debug, warn};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, PM_REMOVE, PeekMessageW, WM_LBUTTONUP, WM_MOUSEMOVE,
};

use super::consts::TICK;
use super::window::PopupWindow;
use crate::enums::{PopupAnchor, PopupUiEvent};
use crate::render::{RenderedPopup, Renderer, hit_row};
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// Popup bottom edge floats this many px above the anchor window's
/// bottom edge (or the screen bottom). Matches the X11 backend.
const BOTTOM_OFFSET: i32 = 96;

#[derive(Debug, Error)]
pub enum WindowsPopupError {
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}

enum Cmd {
    Show(Box<PopupModel>),
    Hide,
}

/// Channel-sending handle; the popup thread owns everything else.
pub struct WindowsPopup {
    cmds: Sender<Cmd>,
    send_failed: AtomicBool,
}

impl WindowsPopup {
    pub fn try_new(events: Sender<PopupUiEvent>) -> Result<Self, WindowsPopupError> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("poltertype-popup-windows".into())
            .spawn(move || run(cmd_rx, events))
            .map_err(WindowsPopupError::Spawn)?;
        Ok(Self {
            cmds: cmd_tx,
            send_failed: AtomicBool::new(false),
        })
    }

    fn send(&self, cmd: Cmd) {
        if self.cmds.send(cmd).is_err() && !self.send_failed.swap(true, Ordering::Relaxed) {
            warn!("popup thread is gone; suggestions will not be shown");
        }
    }
}

impl SuggestionPopup for WindowsPopup {
    fn show(&self, model: PopupModel) {
        self.send(Cmd::Show(Box::new(model)));
    }

    fn hide(&self) {
        self.send(Cmd::Hide);
    }

    fn backend_name(&self) -> &'static str {
        "windows-layered-topmost"
    }
}

/// What is on screen right now.
struct Shown {
    model: PopupModel,
    rendered: RenderedPopup,
    /// Window origin in virtual-screen coordinates.
    origin: (i32, i32),
    scale: f32,
    hover: Option<usize>,
    deadline: Instant,
}

fn run(cmd_rx: Receiver<Cmd>, events: Sender<PopupUiEvent>) {
    let Some(win) = PopupWindow::create() else {
        warn!("could not create the overlay window; suggestions will not be shown");
        return;
    };
    let mut renderer = Renderer::new();
    let mut shown: Option<Shown> = None;

    loop {
        // ── commands from the engine ────────────────────────────────
        loop {
            match cmd_rx.try_recv() {
                Ok(Cmd::Show(model)) => {
                    shown = present(&win, &mut renderer, *model);
                    if shown.is_none() {
                        win.hide();
                    }
                }
                Ok(Cmd::Hide) => {
                    win.hide();
                    shown = None;
                }
                Err(TryRecvError::Empty) => break,
                // The app dropped the handle: take the window down with
                // it rather than leaving an overlay with no owner.
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // ── mouse ───────────────────────────────────────────────────
        pump_messages(&win, &mut renderer, &mut shown, &events);

        // ── self-hide ───────────────────────────────────────────────
        if let Some(view) = &shown
            && Instant::now() >= view.deadline
        {
            let generation = view.model.generation;
            win.hide();
            shown = None;
            let _ = events.send(PopupUiEvent::TimedOut { generation });
        }

        thread::sleep(TICK);
    }
}

/// Render `model`, work out where it goes, and put it on screen.
fn present(win: &PopupWindow, renderer: &mut Renderer, model: PopupModel) -> Option<Shown> {
    // Scale first, from the monitor the anchor is on: the rendered size
    // depends on it, and so does the placement that uses that size.
    let (ax, ay) = anchor_probe(&model.anchor);
    let scale = PopupWindow::scale_at(ax, ay);

    let rendered = renderer.render(&model, None, scale);
    let w = rendered.pixmap.width() as i32;
    let h = rendered.pixmap.height() as i32;
    let origin = place(w, h, &model.anchor);

    if !win.show(rendered.pixmap.data(), w, h, origin.0, origin.1) {
        return None;
    }
    // Deliberately no word in this line — the tooltip's contents are
    // the user's text, and this crate logs none of it.
    debug!(entries = model.entries.len(), w, h, scale, "tooltip shown");
    let deadline = Instant::now() + model.timeout;
    Some(Shown {
        model,
        rendered,
        origin,
        scale,
        hover: None,
        deadline,
    })
}

/// A point on the monitor the tooltip is about to appear on, used only
/// to ask that monitor its scale.
fn anchor_probe(anchor: &PopupAnchor) -> (i32, i32) {
    match *anchor {
        PopupAnchor::Point { x, y, .. } => (x, y),
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => (x + width as i32 / 2, y + height as i32 / 2),
        PopupAnchor::ScreenBottom { .. } => {
            let (vx, vy, vw, vh) = PopupWindow::virtual_screen();
            (vx + vw / 2, vy + vh / 2)
        }
    }
}

/// Virtual-screen placement: the shared side-picker around the caret
/// for `Point`, centred on the anchor window with the bottom edge
/// `BOTTOM_OFFSET` above its bottom for `WindowRect`; clamped to the
/// virtual desktop either way.
///
/// The bounds are the union of every monitor rather than the primary
/// one, so a tooltip near the edge of a second screen slides along that
/// edge instead of being yanked back to the first.
fn place(w: i32, h: i32, anchor: &PopupAnchor) -> (i32, i32) {
    let (vx, vy, vw, vh) = PopupWindow::virtual_screen();
    let (px, py) = match *anchor {
        PopupAnchor::Point { x, y, height, .. } => {
            // `place_near_point` works in a 0-based space; shift the
            // virtual desktop's origin out and back so a left-hand or
            // upper monitor (negative coordinates) is handled.
            let (rx, ry) = crate::place::place_near_point(
                x - vx,
                y - vy,
                y - vy + height as i32,
                w,
                h,
                Some((vw, vh)),
            );
            (rx + vx, ry + vy)
        }
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => (
            x + (width as i32 - w) / 2,
            y + height as i32 - BOTTOM_OFFSET - h,
        ),
        PopupAnchor::ScreenBottom { .. } => (vx + (vw - w) / 2, vy + vh - BOTTOM_OFFSET - h),
    };
    (
        px.clamp(vx, (vx + vw - w).max(vx)),
        py.clamp(vy, (vy + vh - h).max(vy)),
    )
}

/// Drain the thread's message queue, turning mouse messages into hover
/// and accept events. Everything else goes to the default handler.
fn pump_messages(
    win: &PopupWindow,
    renderer: &mut Renderer,
    shown: &mut Option<Shown>,
    events: &Sender<PopupUiEvent>,
) {
    let mut msg = MSG::default();
    // Safety: PeekMessageW on this thread's own queue; `msg` is ours.
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        match msg.message {
            WM_MOUSEMOVE => {
                if let Some(row) = row_at(shown, msg.lParam.0)
                    && let Some(view) = shown.as_mut()
                    && view.hover != row
                {
                    view.hover = row;
                    redraw(win, renderer, view);
                }
            }
            WM_LBUTTONUP => {
                let Some(Some(index)) = row_at(shown, msg.lParam.0) else {
                    continue;
                };
                let Some(view) = shown.as_ref() else { continue };
                let generation = view.model.generation;
                win.hide();
                *shown = None;
                let _ = events.send(PopupUiEvent::Accepted { generation, index });
            }
            _ => {
                // Safety: dispatching a message we just took off our
                // own queue, to our own window procedure.
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                }
            }
        }
    }
}

/// Which row the pointer is over, from a packed mouse `lParam`.
/// `None` means there is nothing shown; `Some(None)` means the pointer
/// is on the panel but not on a row.
fn row_at(shown: &Option<Shown>, lparam: isize) -> Option<Option<usize>> {
    let view = shown.as_ref()?;
    // Low and high words are signed client coordinates.
    let x = (lparam & 0xFFFF) as u16 as i16 as f32;
    let y = ((lparam >> 16) & 0xFFFF) as u16 as i16 as f32;
    Some(hit_row(&view.rendered.rows, x, y))
}

/// Re-render at the current hover state and push the new pixels.
fn redraw(win: &PopupWindow, renderer: &mut Renderer, view: &mut Shown) {
    let rendered = renderer.render(&view.model, view.hover, view.scale);
    let w = rendered.pixmap.width() as i32;
    let h = rendered.pixmap.height() as i32;
    win.show(rendered.pixmap.data(), w, h, view.origin.0, view.origin.1);
    view.rendered = rendered;
}
