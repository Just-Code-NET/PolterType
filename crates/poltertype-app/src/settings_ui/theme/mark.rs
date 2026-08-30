//! The PolterType mark, as an image.
//!
//! The pixels come from `poltertype-icon` — the same rasteriser that
//! draws the window's own icon and the `hicolor` theme entry — so the
//! logo in this window cannot drift away from the one the desktop
//! shows.

use iced::widget::image::Handle;
use iced::widget::{Image, image};
use parking_lot::Mutex;

/// Rasterisations already built, keyed by pixel size. At most two
/// entries (the sidebar's and the About card's), so a linear scan is
/// the whole lookup.
///
/// Cached because `view` is rebuilt on every state change and a fresh
/// `Handle` is a fresh id to the renderer: uncached, each rebuild would
/// re-rasterise the mark *and* make the renderer re-upload it.
static CACHE: Mutex<Vec<(u32, Handle)>> = Mutex::new(Vec::new());

/// The mark at `px` logical pixels.
///
/// An image rather than vectors on a `canvas`: iced's tiny-skia
/// backend applies a canvas frame's clip in the wrong coordinate
/// space, so the mark drew as a fragment in the sidebar and as nothing
/// at all on the About card (issue #49). This path has no clip and is
/// the one the window icon already went through.
///
/// Rasterised at twice the requested size, because `view` is not told
/// the window's scale factor and a logo may be soft but must not be
/// blocky on a 2× display.
pub fn mark(px: u16) -> Image<Handle> {
    Image::new(handle(u32::from(px) * 2))
        .width(f32::from(px))
        .height(f32::from(px))
        .filter_method(image::FilterMethod::Linear)
}

pub(super) fn handle(px: u32) -> Handle {
    let mut cache = CACHE.lock();
    if let Some((_, h)) = cache.iter().find(|(size, _)| *size == px) {
        return h.clone();
    }
    let h = Handle::from_rgba(px, px, poltertype_icon::rasterise(px));
    cache.push((px, h.clone()));
    h
}
