//! Wayland compositor state: the bound globals, the mapped surface (if
//! any), and the methods that create, place and redraw it. Split from
//! [`super::handlers`], which answers the compositor's own callbacks
//! on this same state.

use std::time::Instant;

use crossbeam_channel::Sender;
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::SlotPool;
use tracing::{debug, warn};
use wayland_client::QueueHandle;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_pointer, wl_shm};

use super::enums::{Cmd, WlInitError};
use super::types::TargetOutput;
use super::view::View;
use crate::enums::{PopupAnchor, PopupUiEvent};
use crate::renderer::Renderer;
use crate::types::PopupModel;

/// Popup bottom edge floats this many logical px above the anchor
/// window's bottom edge (or the screen bottom) — the neighbourhood of
/// chat inputs and shell prompts. `Point` anchors use [`crate::place`]
/// instead.
const BOTTOM_OFFSET: i32 = 96;

pub(super) struct WlState {
    pub(super) registry_state: RegistryState,
    pub(super) output_state: OutputState,
    pub(super) seat_state: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    pub(super) shm: Shm,
    pool: SlotPool,
    pub(super) pointer: Option<wl_pointer::WlPointer>,
    pub(super) events: Sender<PopupUiEvent>,
    renderer: Renderer,
    pub(super) view: Option<View>,
}

impl WlState {
    pub(super) fn new(
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

    pub(super) fn handle_cmd(&mut self, cmd: Cmd, qh: &QueueHandle<Self>) {
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

        let target = self.target_output(&model.anchor);
        let scale = target
            .as_ref()
            .map_or_else(|| self.sharpest_scale(), |t| t.scale);
        let rendered = self.renderer.render(&model, None, scale as f32);
        // The renderer keeps device size an exact multiple of the
        // integer scale, so this division is lossless.
        let logical_w = rendered.pixmap.width() / scale as u32;
        let logical_h = rendered.pixmap.height() / scale as u32;
        let placement = target
            .as_ref()
            .and_then(|t| place_on_output(&model.anchor, t, logical_w as i32, logical_h as i32));

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("poltertype-suggestions"),
            // Pinned to the output the placement was computed for, and
            // to nothing at all otherwise: margins mean nothing on a
            // screen we did not measure.
            placement.and(target.as_ref()).map(|t| &t.output),
        );
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(logical_w, logical_h);

        match placement {
            Some((local_x, local_y)) => {
                // The placement is measured against the whole output,
                // so the margins must be too — left at the default the
                // compositor measures them from whatever a panel's
                // exclusive zone leaves over, sliding the tooltip by
                // the height of the user's bar.
                layer.set_exclusive_zone(-1);
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
            output = ?target.as_ref().and_then(|t| self.output_state.info(&t.output)).and_then(|i| i.name),
            output_rect = ?target.as_ref().map(|t| (t.origin, t.size)),
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

    /// The output the anchor points into, when the compositor has said
    /// where its outputs are.
    ///
    /// Resolved from the anchor's own global coordinates rather than
    /// from a name the caller looked up elsewhere: this list is what
    /// the margins are measured against, and a second source of truth
    /// for the same layout can only ever disagree with it.
    fn target_output(&self, anchor: &PopupAnchor) -> Option<TargetOutput> {
        let (x, y) = anchor_point(anchor)?;
        self.output_state.outputs().find_map(|output| {
            let info = self.output_state.info(&output)?;
            let origin = info.logical_position?;
            let size = info.logical_size?;
            let inside =
                x >= origin.0 && x < origin.0 + size.0 && y >= origin.1 && y < origin.1 + size.1;
            inside.then_some(TargetOutput {
                output,
                origin,
                size,
                scale: info.scale_factor.max(1),
            })
        })
    }

    /// Scale to render at when the compositor will pick the output:
    /// the sharpest one, so the tooltip is never blurry wherever it
    /// lands.
    fn sharpest_scale(&self) -> i32 {
        self.output_state
            .outputs()
            .filter_map(|o| self.output_state.info(&o))
            .map(|info| info.scale_factor)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// Upload the rendered pixmap. Only valid once configured.
    pub(super) fn draw(&mut self) {
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
    pub(super) fn set_hover(&mut self, hover: Option<usize>) {
        let Some(view) = &mut self.view else { return };
        if view.hover == hover {
            return;
        }
        view.hover = hover;
        view.rendered = self.renderer.render(&view.model, hover, view.scale as f32);
        self.draw();
    }

    pub(super) fn check_deadline(&mut self) {
        let Some(view) = &self.view else { return };
        if Instant::now() < view.deadline {
            return;
        }
        let generation = view.model.generation;
        self.view = None;
        let _ = self.events.send(PopupUiEvent::TimedOut { generation });
    }
}

/// The global point that decides which output the tooltip belongs to:
/// the caret itself, or — for a window anchor — the spot near the
/// window's bottom edge the tooltip is about to occupy, which is what
/// matters for a window straddling two screens.
fn anchor_point(anchor: &PopupAnchor) -> Option<(i32, i32)> {
    match *anchor {
        PopupAnchor::Point { x, y, .. } => Some((x, y)),
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
        } => Some((
            x + width as i32 / 2,
            (y + height as i32 - BOTTOM_OFFSET).max(y),
        )),
        PopupAnchor::ScreenBottom => None,
    }
}

/// Top-left of a `w`×`h` tooltip in `target`-local logical pixels, the
/// space layer-shell margins are measured in.
fn place_on_output(
    anchor: &PopupAnchor,
    target: &TargetOutput,
    w: i32,
    h: i32,
) -> Option<(i32, i32)> {
    let (ox, oy) = target.origin;
    let (bw, bh) = target.size;
    match *anchor {
        PopupAnchor::Point { x, y, height } => Some(crate::place::place_near_point(
            x - ox,
            y - oy,
            y - oy + height as i32,
            w,
            h,
            Some(target.size),
        )),
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
        } => Some((
            ((x - ox) + (width as i32 - w) / 2).clamp(0, (bw - w).max(0)),
            ((y - oy) + height as i32 - BOTTOM_OFFSET - h).clamp(0, (bh - h).max(0)),
        )),
        PopupAnchor::ScreenBottom => None,
    }
}
