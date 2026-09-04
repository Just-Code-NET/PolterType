//! Rendering [`PanelState`] onto the panel: initial show, teardown,
//! and the hover repaint. `cg_image` and `mac_hint` are its two small
//! helpers.

use std::sync::Arc;

use core_graphics::color_space::CGColorSpace;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::image::{CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::runtime::AnyObject;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use tracing::{debug, warn};

use crate::types::PopupModel;

use super::callbacks::timeout_fired;
use super::geometry::{place, primary_height, scale_at};
use super::state::PanelState;
use super::types::Shown;

impl PanelState {
    /// Render `model`, work out where it goes, and put it on screen.
    /// Mirrors `present` in the Windows backend.
    pub(super) fn show(&mut self, mut model: PopupModel, mtm: MainThreadMarker) {
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

    pub(super) fn hide(&mut self) {
        self.panel.orderOut(None);
        self.shown = None;
    }

    /// Re-render at the current hover state and push the new pixels.
    pub(super) fn redraw_hover(&mut self, hover: Option<usize>) {
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

/// The accept-chord hint in macOS shortcut notation: the config's
/// `"Ctrl+Shift"` reads as a Windows chord to a Mac user; the same
/// keys are `⌃⇧` here. Unknown tokens are dropped rather than shown
/// half-translated.
pub(crate) fn mac_hint(hint: &str) -> String {
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
