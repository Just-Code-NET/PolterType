//! The tray icon as pixels, before any platform has an opinion.

use crate::TrayError;

/// A rasterised tray icon: RGBA8, row-major, no padding.
///
/// The rasteriser lives in the binary, and the two backends want the
/// pixels in different shapes — Windows and macOS hand them to
/// `tray-icon`, Linux writes a PNG for the indicator to read back — so
/// this carries them across without committing to either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl Icon {
    /// Take ownership of a `width`×`height` RGBA buffer.
    ///
    /// # Errors
    ///
    /// [`TrayError::IconSize`] when the buffer is not exactly four
    /// bytes per pixel.
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, TrayError> {
        let want = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if rgba.len() != want {
            return Err(TrayError::IconSize {
                width,
                height,
                want,
                got: rgba.len(),
            });
        }
        Ok(Self {
            rgba,
            width,
            height,
        })
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}
