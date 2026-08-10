//! The AppKit side of the macOS tooltip: a borderless, non-activating
//! `NSPanel` whose content view's layer carries the rendered frame.
//!
//! Everything in this file runs on the main thread, dispatched here
//! from the handle in [`super::popup`] via the main dispatch queue —
//! AppKit's threading rule, and the reason this backend has no thread
//! of its own. State lives in the `STATE` thread-local; every entry
//! point re-acquires the main-thread marker rather than trusting the
//! caller.
//!
//! ## Coordinate spaces
//!
//! Anchors arrive in Core Graphics / accessibility coordinates
//! (global, top-left origin, y down) and all placement maths happens
//! there, so the shared [`crate::place`] logic works unmodified. The
//! single conversion to AppKit's bottom-left space happens when the
//! panel frame is set: `appkit_y = primary_height - cg_y - height`.
//!
//! ## Focus
//!
//! `NSWindowStyleMask::NonactivatingPanel` is the whole "never steal
//! focus" guarantee: the panel can be clicked (row hit-tests run in
//! `mouseDown`) but can never become key, so the editor keeps the
//! keyboard. `setReleasedWhenClosed(false)` matches the lifetime of
//! every other object here: ours, for the process duration.

use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use core_graphics::color_space::CGColorSpace;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::display::CGDisplay;
use core_graphics::image::{CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use crossbeam_channel::Sender;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSPanel, NSScreen, NSStatusWindowLevel, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use tracing::{debug, warn};

use crate::enums::{PopupAnchor, PopupUiEvent};
use crate::render::{RenderedPopup, Renderer, hit_row};
use crate::types::PopupModel;

/// Popup bottom edge floats this many px above the anchor window's
/// bottom edge (or the screen bottom). Matches the other backends.
const BOTTOM_OFFSET: i32 = 96;

/// The app halves of every `PopupUiEvent` flow back through this.
/// Set once at construction; the panel outlives it.
static EVENTS: OnceLock<Sender<PopupUiEvent>> = OnceLock::new();

pub(super) fn register_events(events: Sender<PopupUiEvent>) {
    // A second registration means a second popup handle — the tests do
    // that; the first sender is as good as any.
    let _ = EVENTS.set(events);
}

/// What is on screen right now.
struct Shown {
    model: PopupModel,
    rendered: RenderedPopup,
    /// Device scale the frame was rendered at (Retina = 2.0).
    scale: f64,
    hover: Option<usize>,
}

/// Panel + renderer + whatever is displayed. Main-thread only.
struct PanelState {
    panel: Retained<NSPanel>,
    view: Retained<PopupView>,
    renderer: Renderer,
    shown: Option<Shown>,
}

thread_local! {
    static STATE: RefCell<Option<PanelState>> = const { RefCell::new(None) };
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

/// The self-hide timer firing. Stale timers (a newer offer replaced
/// the one that scheduled them) match no generation and do nothing.
fn timeout_fired(generation: u64) {
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

impl PanelState {
    fn create(mtm: MainThreadMarker) -> Option<Self> {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setLevel(NSStatusWindowLevel);
        // Visible on every space, unmoved by space switches, out of the
        // Cmd+Tab window cycle — the macOS spelling of WS_EX_TOOLWINDOW.
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setIgnoresMouseEvents(false);
        // We create and own it; AppKit must not free it under us.
        unsafe { panel.setReleasedWhenClosed(false) };

        let view = PopupView::new(mtm);
        panel.setContentView(Some(&view));

        Some(Self {
            panel,
            view,
            renderer: Renderer::new(),
            shown: None,
        })
    }

    /// Render `model`, work out where it goes, and put it on screen.
    /// Mirrors `present` in the Windows backend.
    fn show(&mut self, mut model: PopupModel, mtm: MainThreadMarker) {
        // The hint arrives as a config string ("Ctrl+Shift"); show it
        // the way macOS users read shortcuts.
        model.accept_hint = model.accept_hint.map(|h| mac_hint(&h));
        let scale = scale_at(mtm, &model.anchor);
        let rendered = self.renderer.render(&model, None, scale as f32);
        let w_px = rendered.pixmap.width() as f64;
        let h_px = rendered.pixmap.height() as f64;
        let (w, h) = (w_px / scale, h_px / scale);

        let (x, y) = place(w, h, &model.anchor);
        let Some(image) = cg_image(&rendered.pixmap) else {
            warn!("could not build the CGImage; not showing the tooltip");
            self.hide();
            return;
        };

        self.view
            .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h)));
        if let Some(layer) = self.view.layer() {
            // Safety: the layer retains its contents; the CGImage
            // outlives the call. CGImage is the documented contents
            // type for CALayer despite the `id` signature.
            unsafe { layer.setContents(Some(&*std::ptr::from_ref(&*image).cast::<AnyObject>())) };
            layer.setContentsScale(scale);
        }

        // CG (top-left origin) → AppKit (bottom-left origin).
        let appkit_y = primary_height(mtm) - y - h;
        self.panel.setFrame_display(
            NSRect::new(NSPoint::new(x, appkit_y), NSSize::new(w, h)),
            true,
        );
        self.panel.orderFrontRegardless();

        let generation = model.generation;
        let timeout = model.timeout;
        // Deliberately no word in this line — the tooltip's contents
        // are the user's text, and this crate logs none of it.
        debug!(
            entries = model.entries.len(),
            scale, x, y, w, h, "tooltip shown"
        );
        self.shown = Some(Shown {
            model,
            rendered,
            scale,
            hover: None,
        });

        let when = match DispatchTime::try_from(timeout) {
            Ok(t) => t,
            Err(()) => {
                warn!("tooltip timeout out of dispatch range; showing without a timer");
                return;
            }
        };
        let _ = DispatchQueue::main().after(when, move || timeout_fired(generation));
    }

    fn hide(&mut self) {
        self.panel.orderOut(None);
        self.shown = None;
    }

    /// Re-render at the current hover state and push the new pixels.
    fn redraw_hover(&mut self, hover: Option<usize>) {
        let Some(shown) = &mut self.shown else { return };
        if shown.hover == hover {
            return;
        }
        shown.hover = hover;
        let rendered = self
            .renderer
            .render(&shown.model, shown.hover, shown.scale as f32);
        if let Some(image) = cg_image(&rendered.pixmap)
            && let Some(layer) = self.view.layer()
        {
            // Safety: as in `show`.
            unsafe { layer.setContents(Some(&*std::ptr::from_ref(&*image).cast::<AnyObject>())) };
        }
        shown.rendered = rendered;
    }
}

/// A click on the panel. Accepts the row under the pointer, if any;
/// a click on panel padding is ignored (same as the other backends).
fn click_at(point: NSPoint) {
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

fn hover_at(point: Option<NSPoint>) {
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

/// The rendered frame as a CGImage. `tiny_skia`'s premultiplied RGBA
/// is `kCGBitmapByteOrder32Big | kCGImageAlphaPremultipliedLast`
/// byte-for-byte — no channel swap, unlike the Windows backend.
fn cg_image(pixmap: &tiny_skia::Pixmap) -> Option<CGImage> {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let provider = CGDataProvider::from_buffer(Arc::new(pixmap.data().to_vec()));
    let space = CGColorSpace::create_device_rgb();
    let info = CGImageByteOrderInfo::CGImageByteOrder32Big as u32
        | CGImageAlphaInfo::CGImageAlphaPremultipliedLast as u32;
    Some(CGImage::new(
        w,
        h,
        8,
        32,
        w * 4,
        &space,
        info,
        &provider,
        false,
        0, // kCGRenderingIntentDefault
    ))
}

/// Height of the primary screen — the flip reference between the CG
/// and AppKit spaces.
fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm)
        .firstObject()
        .map_or(0.0, |s| s.frame().size.height)
}

/// The scale the renderer should draw at for the anchor's screen.
/// Asked per screen via `backingScaleFactor` — the macOS spelling of
/// the Windows per-monitor DPI query.
fn scale_at(mtm: MainThreadMarker, anchor: &PopupAnchor) -> f64 {
    let (ax, ay) = anchor_point(anchor);
    let primary_h = primary_height(mtm);
    // Anchor in AppKit coordinates for the frame contains-point test.
    let appkit = NSPoint::new(ax, primary_h - ay);
    for screen in NSScreen::screens(mtm).iter() {
        if point_in_rect(appkit, screen.frame()) {
            return screen.backingScaleFactor();
        }
    }
    1.0
}

/// A point on the display the tooltip is about to appear on (CG
/// space), used to ask that display its scale.
fn anchor_point(anchor: &PopupAnchor) -> (f64, f64) {
    match *anchor {
        PopupAnchor::Point { x, y, .. } => (x as f64, y as f64),
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => (
            (x + width as i32 / 2) as f64,
            (y + height as i32 / 2) as f64,
        ),
        PopupAnchor::ScreenBottom { .. } => {
            let (vx, vy, vw, vh) = display_union();
            (vx + vw / 2.0, vy + vh / 2.0)
        }
    }
}

/// The union of every active display's bounds, in CG global
/// coordinates — the same role `GetSystemMetrics(SM_*VIRTUALSCREEN)`
/// plays in the Windows backend.
fn display_union() -> (f64, f64, f64, f64) {
    let Ok(ids) = CGDisplay::active_displays() else {
        let b = CGDisplay::main().bounds();
        return (b.origin.x, b.origin.y, b.size.width, b.size.height);
    };
    let mut union: Option<(f64, f64, f64, f64)> = None;
    for id in ids {
        let b = CGDisplay::new(id).bounds();
        union = Some(match union {
            None => (b.origin.x, b.origin.y, b.size.width, b.size.height),
            Some((x, y, w, h)) => {
                let (x1, y1) = (x.min(b.origin.x), y.min(b.origin.y));
                let (x2, y2) = (
                    (x + w).max(b.origin.x + b.size.width),
                    (y + h).max(b.origin.y + b.size.height),
                );
                (x1, y1, x2 - x1, y2 - y1)
            }
        });
    }
    union.unwrap_or_else(|| {
        let b = CGDisplay::main().bounds();
        (b.origin.x, b.origin.y, b.size.width, b.size.height)
    })
}

/// Placement in CG coordinates: the shared side-picker around the
/// caret for `Point`, centred on the anchor window with the bottom
/// edge `BOTTOM_OFFSET` above its bottom for `WindowRect`; clamped to
/// the display union either way. Mirrors `place` in the Windows
/// backend.
fn place(w: f64, h: f64, anchor: &PopupAnchor) -> (f64, f64) {
    let (vx, vy, vw, vh) = display_union();
    let (wi, hi) = (w.ceil() as i32, h.ceil() as i32);
    let (px, py) = match *anchor {
        PopupAnchor::Point { x, y, height, .. } => {
            // `place_near_point` works in a 0-based space; shift the
            // union's origin out and back so a left-hand or upper
            // display (negative coordinates) is handled.
            let (rx, ry) = crate::place::place_near_point(
                x - vx as i32,
                y - vy as i32,
                y - vy as i32 + height as i32,
                wi,
                hi,
                Some((vw as i32, vh as i32)),
            );
            (rx as f64 + vx, ry as f64 + vy)
        }
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => (
            x as f64 + (width as f64 - w) / 2.0,
            y as f64 + height as f64 - BOTTOM_OFFSET as f64 - h,
        ),
        PopupAnchor::ScreenBottom { .. } => {
            (vx + (vw - w) / 2.0, vy + vh - BOTTOM_OFFSET as f64 - h)
        }
    };
    (
        px.clamp(vx, (vx + vw - w).max(vx)),
        py.clamp(vy, (vy + vh - h).max(vy)),
    )
}

/// `NSMouseInRect` spelled in Rust — one less AppKit import.
fn point_in_rect(point: NSPoint, rect: NSRect) -> bool {
    point.x >= rect.origin.x
        && point.x < rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y < rect.origin.y + rect.size.height
}

/// The accept-chord hint in macOS shortcut notation: the config's
/// `"Ctrl+Shift"` reads as a Windows chord to a Mac user; the same
/// keys are `⌃⇧` here. Unknown tokens are dropped rather than shown
/// half-translated.
fn mac_hint(hint: &str) -> String {
    hint.split('+')
        .filter_map(|token| match token.trim().to_lowercase().as_str() {
            "ctrl" | "control" => Some('⌃'),
            "shift" => Some('⇧'),
            "alt" | "option" => Some('⌥'),
            "cmd" | "command" | "meta" | "super" | "win" => Some('⌘'),
            _ => None,
        })
        .collect()
}

#[derive(Default)]
struct PopupViewIvars;

define_class!(
    // Safety:
    // - `NSView` has no subclassing requirements relevant here.
    // - The class is main-thread-only, matching AppKit's rules, and
    //   is never subclassed further.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "PolterTypePopupView"]
    #[ivars = PopupViewIvars]
    struct PopupView;

    impl PopupView {
        // Top-down coordinates, so the shared hit-test works on the
        // renderer's row rectangles without a flip.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            click_at(point);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            hover_at(Some(point));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            hover_at(None);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            // Safety: chaining to super is required by AppKit.
            let () = unsafe { msg_send![super(self), updateTrackingAreas] };
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    NSTrackingAreaOptions::MouseEnteredAndExited
                        | NSTrackingAreaOptions::MouseMoved
                        | NSTrackingAreaOptions::InVisibleRect
                        // The panel is never key; without this the
                        // hover events would never fire.
                        | NSTrackingAreaOptions::ActiveAlways,
                    // Safety: same object pointer, retyped; the area
                    // retains its owner.
                    Some(&*std::ptr::from_ref(self).cast::<AnyObject>()),
                    None,
                )
            };
            self.addTrackingArea(&area);
        }
    }
);

impl PopupView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PopupViewIvars);
        // Safety: standard NSView initialisation of our own subclass.
        unsafe { msg_send![super(this), init] }
    }
}
