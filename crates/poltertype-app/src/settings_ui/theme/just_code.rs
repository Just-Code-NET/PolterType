//! The just-code emblem, as an image.
//!
//! Unlike PolterType's own mark this is somebody else's artwork rather
//! than geometry we own, so it ships as the two small PNGs next to the
//! crate, cut from the 1000 px original: the second lifts the greys for
//! a dark window and leaves the brand cyan untouched, the same treatment
//! the website gives the wordmark.

use iced::widget::image::Handle;
use iced::widget::{Image, image};
use parking_lot::Mutex;
use tracing::warn;

use super::consts::JUST_CODE_MARK_PX;

static LIGHT_PNG: &[u8] = include_bytes!("../../../assets/just-code-mark.png");
static DARK_PNG: &[u8] = include_bytes!("../../../assets/just-code-mark-dark.png");

/// Decoded once per variant, for the reason `mark` caches: a fresh
/// `Handle` per view rebuild is a fresh upload to the renderer.
static CACHE: Mutex<[Option<Handle>; 2]> = Mutex::new([None, None]);

/// The emblem at `px` logical pixels. Stored at exactly twice the size it
/// is drawn: a whole-number reduction is what keeps this much fine detail
/// from turning to noise on a 1× display, and it stays sharp on a 2× one.
pub fn just_code_mark(px: u16, dark: bool) -> Image<Handle> {
    let mut cache = CACHE.lock();
    let slot = &mut cache[usize::from(dark)];
    let handle = slot
        .get_or_insert_with(|| {
            let png = if dark { DARK_PNG } else { LIGHT_PNG };
            let rgba = decode(png).unwrap_or_else(|e| {
                warn!(?e, "could not decode the just-code mark");
                vec![0; (JUST_CODE_MARK_PX * JUST_CODE_MARK_PX * 4) as usize]
            });
            Handle::from_rgba(JUST_CODE_MARK_PX, JUST_CODE_MARK_PX, rgba)
        })
        .clone();
    Image::new(handle)
        .width(f32::from(px))
        .height(f32::from(px))
        .filter_method(image::FilterMethod::Linear)
}

/// `pub(super)` for the test that holds the files to the size above.
pub(super) fn decode(png_bytes: &[u8]) -> Result<Vec<u8>, png::DecodingError> {
    let mut reader = png::Decoder::new(png_bytes).read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut rgba)?;
    Ok(rgba)
}

#[cfg(test)]
pub(super) fn embedded() -> [&'static [u8]; 2] {
    [LIGHT_PNG, DARK_PNG]
}
