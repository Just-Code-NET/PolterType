//! X11 backend: an override-redirect window on the root. The WM never
//! manages (or focuses) such windows, which gives us the "never steal
//! keyboard focus" guarantee for free — and needs no permissions.
//!
//! One dedicated thread owns the connection and the window; the public
//! handle only pushes commands into a channel. Parked on the channel
//! while hidden; ~16 ms tick with `poll_for_event` while mapped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use thiserror::Error;
use tracing::{debug, warn};
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ColormapAlloc, ConfigureWindowAux, ConnectionExt, CreateGCAux, CreateWindowAux,
    EventMask, Gcontext, ImageFormat, ImageOrder, PropMode, StackMode, VisualClass, Visualid,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::enums::{PopupAnchor, PopupUiEvent};
use crate::render::{RenderedPopup, Renderer, hit_row};
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// Popup bottom edge floats this many px above the anchor window's
/// bottom edge (or the screen bottom).
const BOTTOM_OFFSET: i32 = 96;
/// Tick period while the window is mapped.
const TICK: Duration = Duration::from_millis(16);
/// Opaque panel background for servers without a 32-bit ARGB visual.
const OPAQUE_BG: (u8, u8, u8) = (0x16, 0x16, 0x1E);

#[derive(Debug, Error)]
pub enum X11PopupError {
    #[error("x11 connect: {0}")]
    Connect(#[from] x11rb::errors::ConnectError),
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}

enum Cmd {
    Show(PopupModel),
    Hide,
}

/// Channel-sending handle; the X11 thread owns everything else.
pub struct X11Popup {
    cmds: Sender<Cmd>,
    send_failed: AtomicBool,
}

impl X11Popup {
    pub fn try_new(events: Sender<PopupUiEvent>) -> Result<Self, X11PopupError> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("poltertype-popup-x11".into())
            .spawn(move || run(conn, screen_num, cmd_rx, events))
            .map_err(X11PopupError::Spawn)?;
        Ok(Self {
            cmds: cmd_tx,
            send_failed: AtomicBool::new(false),
        })
    }

    fn send(&self, cmd: Cmd) {
        if self.cmds.send(cmd).is_err() && !self.send_failed.swap(true, Ordering::Relaxed) {
            warn!("x11 popup thread is gone; suggestions will not be shown");
        }
    }
}

impl SuggestionPopup for X11Popup {
    fn show(&self, model: PopupModel) {
        self.send(Cmd::Show(model));
    }

    fn hide(&self) {
        self.send(Cmd::Hide);
    }

    fn backend_name(&self) -> &'static str {
        "linux-x11-override-redirect"
    }
}

/// EWMH atoms, resolved once. Best-effort: a server without them just
/// skips the hints (override-redirect works regardless).
struct Atoms {
    window_type: Atom,
    window_type_tooltip: Atom,
    wm_state: Atom,
    wm_state_above: Atom,
}

/// The depth/visual the popup window is created with. 32-bit TrueColor
/// when the server offers one (real transparency under a compositor);
/// otherwise the root visual with an opaque panel.
struct VisualPick {
    depth: u8,
    visual: Visualid,
    colormap: Option<u32>,
    argb: bool,
}

/// The currently mapped window and what it shows.
struct WinView {
    window: Window,
    gc: Gcontext,
    rendered: RenderedPopup,
    model: PopupModel,
    hover: Option<usize>,
    deadline: Instant,
}

struct X11State {
    conn: RustConnection,
    root: Window,
    screen_w: u16,
    screen_h: u16,
    visual: VisualPick,
    lsb_first: bool,
    atoms: Option<Atoms>,
    events: Sender<PopupUiEvent>,
    renderer: Renderer,
    win: Option<WinView>,
}

fn run(
    conn: RustConnection,
    screen_num: usize,
    cmd_rx: Receiver<Cmd>,
    events: Sender<PopupUiEvent>,
) {
    let mut state = match X11State::new(conn, screen_num, events) {
        Some(state) => state,
        None => {
            warn!("x11 popup thread failed to initialise");
            return;
        }
    };

    loop {
        if state.win.is_some() {
            loop {
                match cmd_rx.try_recv() {
                    Ok(cmd) => state.handle_cmd(cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if !state.pump_events() {
                return;
            }
            state.check_deadline();
            thread::sleep(TICK);
        } else {
            match cmd_rx.recv() {
                Ok(cmd) => state.handle_cmd(cmd),
                Err(_) => return,
            }
        }
    }
}

impl X11State {
    fn new(conn: RustConnection, screen_num: usize, events: Sender<PopupUiEvent>) -> Option<Self> {
        let setup = conn.setup();
        let lsb_first = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let screen = setup.roots.get(screen_num)?;
        let (root, screen_w, screen_h) =
            (screen.root, screen.width_in_pixels, screen.height_in_pixels);
        let (root_visual, root_depth) = (screen.root_visual, screen.root_depth);

        // Prefer a 32-bit TrueColor visual (real transparency); fall
        // back to the root visual and an opaque panel.
        let argb_visual = screen
            .allowed_depths
            .iter()
            .filter(|d| d.depth == 32)
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.class == VisualClass::TRUE_COLOR)
            .map(|v| v.visual_id);
        // We upload 4 bytes per pixel; every current server advertises
        // 32 bpp for both depth-24 and depth-32 ZPixmaps.
        let bpp_ok = |depth: u8| {
            setup
                .pixmap_formats
                .iter()
                .any(|f| f.depth == depth && f.bits_per_pixel == 32)
        };
        let visual = match argb_visual {
            Some(visual) if bpp_ok(32) => {
                let colormap = conn.generate_id().ok()?;
                conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual)
                    .ok()?;
                VisualPick {
                    depth: 32,
                    visual,
                    colormap: Some(colormap),
                    argb: true,
                }
            }
            _ => {
                if !bpp_ok(root_depth) {
                    warn!(
                        depth = root_depth,
                        "no 32-bpp ZPixmap format for root depth"
                    );
                    return None;
                }
                VisualPick {
                    depth: root_depth,
                    visual: root_visual,
                    colormap: None,
                    argb: false,
                }
            }
        };

        let atoms = resolve_atoms(&conn);
        Some(Self {
            conn,
            root,
            screen_w,
            screen_h,
            visual,
            lsb_first,
            atoms,
            events,
            renderer: Renderer::new(),
            win: None,
        })
    }

    fn handle_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Show(model) => {
                if let Err(e) = self.show(model) {
                    warn!(err = %e, "x11 popup show failed");
                    self.destroy_window();
                }
            }
            Cmd::Hide => self.destroy_window(),
        }
    }

    /// Create, hint, map and paint a window for `model`.
    /// Destroy-and-recreate per show avoids resize handling and stale
    /// state.
    fn show(&mut self, model: PopupModel) -> Result<(), ReplyOrIdError> {
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

    /// Upload the rendered pixmap. One `PutImage` — the popup is tiny.
    fn paint(&self) -> Result<(), ReplyOrIdError> {
        let Some(view) = &self.win else {
            return Ok(());
        };
        let w = view.rendered.pixmap.width() as u16;
        let h = view.rendered.pixmap.height() as u16;
        let data = self.upload_bytes(&view.rendered.pixmap);
        self.conn.put_image(
            ImageFormat::Z_PIXMAP,
            view.window,
            view.gc,
            w,
            h,
            0,
            0,
            0,
            self.visual.depth,
            &data,
        )?;
        Ok(())
    }

    /// tiny-skia premultiplied RGBA → server pixel bytes.
    ///
    /// 32-bit ARGB visuals conventionally expect premultiplied alpha
    /// when a compositor runs, so the bytes pass through reordered.
    /// For 24-bit, composite over the opaque panel colour first.
    fn upload_bytes(&self, pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
        let src = pixmap.data();
        let mut out = Vec::with_capacity(src.len());
        for px in src.chunks_exact(4) {
            let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
            let (r, g, b, a) = if self.visual.argb {
                (r, g, b, a)
            } else {
                let inv = 255 - u16::from(a);
                (
                    b_add(r, OPAQUE_BG.0, inv),
                    b_add(g, OPAQUE_BG.1, inv),
                    b_add(b, OPAQUE_BG.2, inv),
                    0,
                )
            };
            if self.lsb_first {
                out.extend_from_slice(&[b, g, r, a]);
            } else {
                out.extend_from_slice(&[a, r, g, b]);
            }
        }
        out
    }

    /// Drain the X event queue. Returns `false` when the connection is
    /// gone and the thread should exit.
    fn pump_events(&mut self) -> bool {
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

    fn check_deadline(&mut self) {
        let Some(view) = &self.win else { return };
        if Instant::now() < view.deadline {
            return;
        }
        let generation = view.model.generation;
        self.destroy_window();
        let _ = self.events.send(PopupUiEvent::TimedOut { generation });
    }

    fn destroy_window(&mut self) {
        if let Some(view) = self.win.take() {
            let _ = self.conn.free_gc(view.gc);
            let _ = self.conn.destroy_window(view.window);
            let _ = self.conn.flush();
        }
    }
}

fn resolve_atoms(conn: &RustConnection) -> Option<Atoms> {
    let intern = |name: &str| -> Option<Atom> {
        conn.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    Some(Atoms {
        window_type: intern("_NET_WM_WINDOW_TYPE")?,
        window_type_tooltip: intern("_NET_WM_WINDOW_TYPE_TOOLTIP")?,
        wm_state: intern("_NET_WM_STATE")?,
        wm_state_above: intern("_NET_WM_STATE_ABOVE")?,
    })
}

/// `src + bg × inv / 255` for one premultiplied channel.
fn b_add(src: u8, bg: u8, inv: u16) -> u8 {
    src.saturating_add(((u16::from(bg) * inv + 127) / 255) as u8)
}
