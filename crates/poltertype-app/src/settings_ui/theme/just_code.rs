//! The just-code logo, as an image.
//!
//! Unlike PolterType's own mark this is somebody else's artwork rather
//! than geometry we own, so it ships as two small PNGs next to the crate,
//! rendered from the vector original: the second lifts the greys for a
//! dark window and leaves the brand cyan untouched, the same treatment the
//! website gives the same wordmark.

use iced::widget::image::Handle;
use iced::widget::{Image, image};
use parking_lot::Mutex;
use tracing::warn;

use super::consts::{JUST_CODE_LOGO_HEIGHT, JUST_CODE_LOGO_WIDTH};

static LIGHT_PNG: &[u8] = include_bytes!("../../../assets/just-code-logo.png");
static DARK_PNG: &[u8] = include_bytes!("../../../assets/just-code-logo-dark.png");

/// Decoded once per variant, for the reason `mark` caches: a fresh
/// `Handle` per view rebuild is a fresh upload to the renderer.
static CACHE: Mutex<[Option<Handle>; 2]> = Mutex::new([None, None]);

/// The logo at its one drawn size. Stored at exactly twice that: a
/// whole-number reduction is what keeps this much fine detail from turning
/// to noise on a 1× display, and it stays sharp on a 2× one.
pub fn just_code_logo(dark: bool) -> Image<Handle> {
    let mut cache = CACHE.lock();
    let slot = &mut cache[usize::from(dark)];
    let handle = slot
        .get_or_insert_with(|| {
            let png = if dark { DARK_PNG } else { LIGHT_PNG };
            let rgba = decode(png).unwrap_or_else(|e| {
                warn!(?e, "could not decode the just-code logo");
                vec![0; (JUST_CODE_LOGO_WIDTH * JUST_CODE_LOGO_HEIGHT * 4) as usize]
            });
            Handle::from_rgba(JUST_CODE_LOGO_WIDTH, JUST_CODE_LOGO_HEIGHT, rgba)
        })
        .clone();
    Image::new(handle)
        .width((JUST_CODE_LOGO_WIDTH / 2) as f32)
        .height((JUST_CODE_LOGO_HEIGHT / 2) as f32)
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
