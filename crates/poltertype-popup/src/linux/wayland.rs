//! Wayland backend: a `wlr-layer-shell` overlay surface with
//! `keyboard_interactivity = None`, so the popup can never steal the
//! keys it exists to fix. Works on wlroots compositors (Hyprland,
//! Sway); GNOME/KDE expose no layer-shell to third parties — detected
//! at connect time so the factory can fall through to X11/noop.
//!
//! All Wayland state lives on one dedicated thread; the public handle
//! only pushes commands into a channel. The thread parks on the
//! channel while hidden (zero CPU) and ticks at ~16 ms while a surface
//! is mapped, manually pumping the queue (no calloop).

use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::seat::pointer::{
    BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{CreatePoolError, Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
};
use thiserror::Error;
use tracing::{debug, warn};
use wayland_client::backend::WaylandError;
use wayland_client::globals::{BindError, GlobalError, GlobalList, registry_queue_init};
use wayland_client::protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{ConnectError, Connection, DispatchError, EventQueue, QueueHandle};

use crate::enums::{PopupAnchor, PopupUiEvent};
use crate::render::{RenderedPopup, Renderer, hit_row};
use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

/// Popup bottom edge floats this many logical px above the anchor
/// window's bottom edge (or the screen bottom) — the neighbourhood of
/// chat inputs and shell prompts. (`Point` anchors use the shared
/// side-picking algorithm in [`crate::place`] instead.)
const BOTTOM_OFFSET: i32 = 96;
/// Tick period while a surface is mapped.
const TICK: Duration = Duration::from_millis(16);

#[derive(Debug, Error)]
pub enum WaylandPopupError {
    #[error("wayland connect: {0}")]
    Connect(#[from] ConnectError),
    #[error("wayland globals: {0}")]
    Globals(#[from] GlobalError),
    #[error("compositor exposes no zwlr_layer_shell_v1 (GNOME/KDE)")]
    NoLayerShell,
    #[error("spawn popup thread: {0}")]
    Spawn(std::io::Error),
}

enum Cmd {
    Show(PopupModel),
    Hide,
}

/// Failures while binding globals on the popup thread (after the
/// factory already accepted this backend) — logged, never surfaced.
#[derive(Debug, Error)]
enum WlInitError {
    #[error("bind global: {0}")]
    Bind(#[from] BindError),
    #[error("create shm pool: {0}")]
    Pool(#[from] CreatePoolError),
}

/// Channel-sending handle; the Wayland thread owns everything else.
pub struct WaylandPopup {
    cmds: Sender<Cmd>,
    send_failed: AtomicBool,
}

impl WaylandPopup {
    pub fn try_new(events: Sender<PopupUiEvent>) -> Result<Self, WaylandPopupError> {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init::<WlState>(&conn)?;
        // The factory needs "no layer-shell" distinguished from "no
        // Wayland at all" *before* we commit to this backend.
        let has_layer_shell = globals
            .contents()
            .with_list(|list| list.iter().any(|g| g.interface == "zwlr_layer_shell_v1"));
        if !has_layer_shell {
            return Err(WaylandPopupError::NoLayerShell);
        }

        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("poltertype-popup-wl".into())
            .spawn(move || run(globals, event_queue, cmd_rx, events))
            .map_err(WaylandPopupError::Spawn)?;
        Ok(Self {
            cmds: cmd_tx,
            send_failed: AtomicBool::new(false),
        })
    }

    fn send(&self, cmd: Cmd) {
        // The thread only dies on a compositor error; losing a popup
        // then is fine, but say so once.
        if self.cmds.send(cmd).is_err() && !self.send_failed.swap(true, Ordering::Relaxed) {
            warn!("wayland popup thread is gone; suggestions will not be shown");
        }
    }
}

impl SuggestionPopup for WaylandPopup {
    fn show(&self, model: PopupModel) {
        self.send(Cmd::Show(model));
    }

    fn hide(&self) {
        self.send(Cmd::Hide);
    }

    fn backend_name(&self) -> &'static str {
        "linux-wayland-layer-shell"
    }
}

/// Everything shown right now: surface, pixels, hit-boxes, deadline.
/// Dropped whole on hide — destroying the `LayerSurface` (and its
/// inner `wl_surface`) is the simplest correct unmap.
struct View {
    layer: LayerSurface,
    rendered: RenderedPopup,
    model: PopupModel,
    scale: i32,
    hover: Option<usize>,
    /// No buffer may be attached before the first configure.
    configured: bool,
    deadline: Instant,
}

struct WlState {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    pointer: Option<wl_pointer::WlPointer>,
    events: Sender<PopupUiEvent>,
    renderer: Renderer,
    view: Option<View>,
}

fn run(
    globals: GlobalList,
    mut event_queue: EventQueue<WlState>,
    cmd_rx: Receiver<Cmd>,
    events: Sender<PopupUiEvent>,
) {
    let qh = event_queue.handle();
    let mut state = match WlState::new(&globals, &qh, events) {
        Ok(state) => state,
        Err(e) => {
            warn!(err = %e, "wayland popup thread failed to bind globals");
            return;
        }
    };

    loop {
        if state.view.is_some() {
            loop {
                match cmd_rx.try_recv() {
                    Ok(cmd) => serve(&mut state, &mut event_queue, &qh, cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if let Err(e) = pump(&mut event_queue, &mut state) {
                warn!(err = %e, "wayland popup thread lost its connection");
                return;
            }
            state.check_deadline();
            thread::sleep(TICK);
        } else {
            // Push out any pending destroy before parking, then block
            // on the channel — zero CPU while hidden.
            if let Err(e) = pump(&mut event_queue, &mut state) {
                warn!(err = %e, "wayland popup thread lost its connection");
                return;
            }
            match cmd_rx.recv() {
                Ok(cmd) => serve(&mut state, &mut event_queue, &qh, cmd),
                Err(_) => return,
            }
        }
    }
}

/// Run one command, round-tripping the queue before a `Show`.
///
/// Placement needs the outputs' names, logical sizes and scales, and
/// those arrive as *events*, not with the globals — while
/// `registry_queue_init` has answered by the time this thread starts,
/// the `wl_output`/`xdg_output` replies to `OutputState`'s own binds
/// have not. Between popups the thread is parked on the command
/// channel and reads nothing from the socket, so without this the
/// first popup of every session was placed against an empty output
/// list: no bounds to clamp against, and `output: None` on the layer
/// surface, which hands the compositor the choice of monitor. (The
/// second popup onwards worked, because the tick loop had pumped the
/// queue by then — which is exactly why the bug looked intermittent.)
/// Refreshing per show also picks up hotplugs and mode changes that
/// happened while parked. One round-trip per popup, on a thread that
/// has nothing else to do.
fn serve(
    state: &mut WlState,
    queue: &mut EventQueue<WlState>,
    qh: &QueueHandle<WlState>,
    cmd: Cmd,
) {
    if matches!(cmd, Cmd::Show(_)) {
        if let Err(e) = queue.roundtrip(state) {
            warn!(err = %e, "popup output refresh failed; placing with stale output info");
        }
    }
    state.handle_cmd(cmd, qh);
}

/// Non-blocking queue pump: flush requests, read whatever the socket
/// has (tolerating `WouldBlock`), dispatch to handlers.
fn pump(queue: &mut EventQueue<WlState>, state: &mut WlState) -> Result<(), DispatchError> {
    match queue.flush() {
        Ok(()) => {}
        Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
        Err(e) => return Err(DispatchError::Backend(e)),
    }
    if let Some(guard) = queue.prepare_read() {
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(DispatchError::Backend(e)),
        }
    }
    queue.dispatch_pending(state).map(drop)
}

impl WlState {
    fn new(
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        events: Sender<PopupUiEvent>,
    ) -> Result<Self, WlInitError> {
        let shm = Shm::bind(globals, qh)?;
        // Grows on demand; a popup buffer is ~400 KiB at most.
        let pool = SlotPool::new(4096, &shm)?;
        Ok(Self {
            registry_state: RegistryState::new(globals),
            output_state: OutputState::new(globals, qh),
            seat_state: SeatState::new(globals, qh),
            compositor: CompositorState::bind(globals, qh)?,
            layer_shell: LayerShell::bind(globals, qh)?,
            shm,
            pool,
            pointer: None,
            events,
            renderer: Renderer::new(),
            view: None,
        })
    }

    fn handle_cmd(&mut self, cmd: Cmd, qh: &QueueHandle<Self>) {
        match cmd {
            Cmd::Show(model) => self.show(model, qh),
            Cmd::Hide => self.view = None,
        }
    }

    /// Map a fresh layer surface for `model`, replacing any current one.
    fn show(&mut self, model: PopupModel, qh: &QueueHandle<Self>) {
        // Destroy-and-recreate per show: no resize/reposition protocol
        // dance, and layer surfaces are cheap.
        self.view = None;

        let (output, scale) = self.pick_output(&model.anchor);
        let rendered = self.renderer.render(&model, None, scale as f32);
        // The renderer keeps device size an exact multiple of the
        // integer scale, so this division is lossless.
        let logical_w = rendered.pixmap.width() / scale as u32;
        let logical_h = rendered.pixmap.height() / scale as u32;

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("poltertype-suggestions"),
            output.as_ref(),
        );
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(logical_w, logical_h);

        // Output-local top-left position for the anchors that name an
        // exact spot; `None` = bottom-centred on the output.
        let output_size = output
            .as_ref()
            .and_then(|o| self.output_state.info(o))
            .and_then(|info| info.logical_size);
        let placement: Option<(i32, i32)> = match model.anchor {
            // Near the pointer — the caret proxy. The shared
            // side-picker walks above → below → right → left and
            // clamps inside the output.
            PopupAnchor::Point {
                x,
                y,
                height,
                output_x,
                output_y,
                ..
            } => Some(crate::place::place_near_point(
                x - output_x,
                y - output_y,
                y - output_y + height as i32,
                logical_w as i32,
                logical_h as i32,
                output_size,
            )),
            // Horizontally centred on the window, bottom edge
            // BOTTOM_OFFSET above the window's bottom.
            PopupAnchor::WindowRect {
                x,
                y,
                width,
                height,
                output_x,
                output_y,
                ..
            } => {
                let mut local_x = (x - output_x) + (width as i32 - logical_w as i32) / 2;
                let mut local_y = (y - output_y) + height as i32 - BOTTOM_OFFSET - logical_h as i32;
                if let Some((out_w, out_h)) = output_size {
                    local_x = local_x.clamp(0, (out_w - logical_w as i32).max(0));
                    local_y = local_y.clamp(0, (out_h - logical_h as i32).max(0));
                } else {
                    local_x = local_x.max(0);
                    local_y = local_y.max(0);
                }
                Some((local_x, local_y))
            }
            PopupAnchor::ScreenBottom { .. } => None,
        };
        match placement {
            Some((local_x, local_y)) => {
                layer.set_anchor(Anchor::TOP | Anchor::LEFT);
                layer.set_margin(local_y, 0, 0, local_x);
            }
            None => {
                layer.set_anchor(Anchor::BOTTOM);
                layer.set_margin(0, 0, BOTTOM_OFFSET, 0);
            }
        }
        // Map with an empty commit; the buffer is attached on the
        // compositor's first configure.
        layer.commit();

        debug!(
            entries = model.entries.len(),
            w = rendered.pixmap.width(),
            h = rendered.pixmap.height(),
            scale,
            resolved_output = ?output.as_ref().and_then(|o| self.output_state.info(o)).and_then(|i| i.name),
            ?output_size,
            ?placement,
            "popup surface mapped"
        );
        let deadline = Instant::now() + model.timeout;
        self.view = Some(View {
            layer,
            rendered,
            model,
            scale,
            hover: None,
            configured: false,
            deadline,
        });
    }

    /// Output the anchor names, plus its integer scale (1 if unknown).
    fn pick_output(&self, anchor: &PopupAnchor) -> (Option<wl_output::WlOutput>, i32) {
        let wanted = match anchor {
            PopupAnchor::Point { output, .. }
            | PopupAnchor::WindowRect { output, .. }
            | PopupAnchor::ScreenBottom { output } => output.as_deref(),
        };
        let output = wanted.and_then(|name| {
            self.output_state.outputs().find(|o| {
                self.output_state
                    .info(o)
                    .and_then(|info| info.name)
                    .is_some_and(|n| n == name)
            })
        });
        let scale = match &output {
            Some(o) => self
                .output_state
                .info(o)
                .map(|info| info.scale_factor)
                .unwrap_or(1),
            // Compositor picks the output; render for the sharpest one
            // so we never look blurry there.
            None => self
                .output_state
                .outputs()
                .filter_map(|o| self.output_state.info(&o))
                .map(|info| info.scale_factor)
                .max()
                .unwrap_or(1),
        };
        (output, scale.max(1))
    }

    /// Upload the rendered pixmap. Only valid once configured.
    fn draw(&mut self) {
        let Some(view) = &self.view else { return };
        if !view.configured {
            return;
        }
        let width = view.rendered.pixmap.width() as i32;
        let height = view.rendered.pixmap.height() as i32;
        let (buffer, canvas) =
            match self
                .pool
                .create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
            {
                Ok(pair) => pair,
                Err(e) => {
                    warn!(err = %e, "popup buffer allocation failed");
                    return;
                }
            };
        // tiny-skia premultiplied RGBA → wl_shm ARGB8888 (little-endian
        // B,G,R,A bytes; Wayland expects premultiplied alpha).
        for (src, dst) in view
            .rendered
            .pixmap
            .data()
            .chunks_exact(4)
            .zip(canvas.chunks_exact_mut(4))
        {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        // Pre-version-3 compositors can't scale buffers; they'll show
        // the popup oversized, which is survivable.
        let _ = view.layer.set_buffer_scale(view.scale as u32);
        view.layer.wl_surface().damage_buffer(0, 0, width, height);
        if buffer.attach_to(view.layer.wl_surface()).is_err() {
            warn!("popup buffer attach failed");
            return;
        }
        view.layer.commit();
    }

    /// Re-render on hover change and redraw in place.
    fn set_hover(&mut self, hover: Option<usize>) {
        let Some(view) = &mut self.view else { return };
        if view.hover == hover {
            return;
        }
        view.hover = hover;
        view.rendered = self.renderer.render(&view.model, hover, view.scale as f32);
        self.draw();
    }

    fn check_deadline(&mut self) {
        let Some(view) = &self.view else { return };
        if Instant::now() < view.deadline {
            return;
        }
        let generation = view.model.generation;
        self.view = None;
        let _ = self.events.send(PopupUiEvent::TimedOut { generation });
    }
}

impl CompositorHandler for WlState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // Scale is chosen per show from the target output; a popup that
        // lives ~4 s is not worth live-rescaling.
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Static content: we commit directly, no frame-callback loop.
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for WlState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for WlState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        // Compositor killed the surface (output gone, shell reload…).
        if self.view.as_ref().is_some_and(|v| &v.layer == layer) {
            self.view = None;
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // Fixed-size overlay: we ignore the suggested size and draw our
        // buffer (anchored to at most two edges, the compositor honours
        // the requested size).
        let Some(view) = &mut self.view else { return };
        if &view.layer != layer {
            return;
        }
        view.configured = true;
        self.draw();
    }
}

impl SeatHandler for WlState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        // Pointer only — never bind the keyboard (hard requirement:
        // the popup must not receive, let alone consume, key events).
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for WlState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let Some(view) = &self.view else { return };
            if &event.surface != view.layer.wl_surface() {
                continue;
            }
            // Pointer coords are surface-logical; hit-boxes are device px.
            let scale = view.scale as f64;
            let (px, py) = (
                (event.position.0 * scale) as f32,
                (event.position.1 * scale) as f32,
            );
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let hover = hit_row(&view.rendered.rows, px, py);
                    self.set_hover(hover);
                }
                PointerEventKind::Leave { .. } => {
                    self.set_hover(None);
                }
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => {
                    if let Some(index) = hit_row(&view.rendered.rows, px, py) {
                        let generation = view.model.generation;
                        // Hide first so the popup vanishes the instant
                        // the engine starts retyping.
                        self.view = None;
                        let _ = self
                            .events
                            .send(PopupUiEvent::Accepted { generation, index });
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for WlState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for WlState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(WlState);
delegate_output!(WlState);
delegate_shm!(WlState);
delegate_seat!(WlState);
delegate_pointer!(WlState);
delegate_layer!(WlState);
delegate_registry!(WlState);
