//! Windows `.ico` container writer.
//!
//! An `.ico` is a directory of independent images, and Windows picks
//! whichever entry matches the size it is about to draw. Hand-rolled
//! rather than delegated to `image`, because the format is a header, a
//! table and the images themselves — about eighty lines, against a
//! dependency that decodes a dozen formats we never read.
//!
//! Two encodings live side by side inside one file: PNG for the large
//! entries and raw DIB for the small ones, split at [`ICO_PNG_FROM`].
//! That is the shell's own convention, not ours.

use crate::*;
use std::path::Path;

/// Header of the container, then one 16-byte entry per image.
const DIR_HEADER: usize = 6;
const DIR_ENTRY: usize = 16;

/// `BITMAPINFOHEADER`, the DIB entries' own header.
const BMP_HEADER: usize = 40;

/// Write the app icon to `out` at every size in [`ICO_SIZES`].
pub fn render_ico(out: &Path) -> Result<(), IconError> {
    write_file(out, &encode_ico()?)
}

/// The whole container as bytes, one image per [`ICO_SIZES`] entry.
pub(crate) fn encode_ico() -> Result<Vec<u8>, IconError> {
    let mut images = Vec::with_capacity(ICO_SIZES.len());
    for &size in ICO_SIZES {
        let rgba = rasterise(size);
        let bytes = if size >= ICO_PNG_FROM {
            encode_png(&rgba, size)?
        } else {
            dib(&rgba, size)
        };
        images.push((size, bytes));
    }
    Ok(container(&images))
}

/// The `ICONDIR` header, the `ICONDIRENTRY` table, then the images.
fn container(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let payload: usize = images.iter().map(|(_, b)| b.len()).sum();
    let mut out = Vec::with_capacity(DIR_HEADER + DIR_ENTRY * images.len() + payload);

    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 = icon, 2 = cursor
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = DIR_HEADER + DIR_ENTRY * images.len();
    for (size, bytes) in images {
        // One byte per axis, so 256 is written as 0 — the format's own
        // escape for "the largest size it can name".
        let axis = if *size >= 256 { 0u8 } else { *size as u8 };
        out.extend_from_slice(&[axis, axis, 0, 0]); // w, h, palette, reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += bytes.len();
    }

    for (_, bytes) in images {
        out.extend_from_slice(bytes);
    }
    out
}

/// One image as a 32-bit DIB: header, colour rows, then the AND mask.
///
/// Three traps, all of them silent if you get them wrong: the header
/// declares **twice** the real height (it is describing the colour
/// rows and the mask as one bitmap), the rows run **bottom-up**, and
/// the AND mask is still required even though the alpha channel makes
/// it redundant — several shell paths read it and nothing else.
fn dib(rgba: &[u8], size: u32) -> Vec<u8> {
    let n = size as usize;
    // Mask rows are 1 bit per pixel, padded to a 4-byte boundary.
    let mask_stride = n.div_ceil(32) * 4;

    let mut out = Vec::with_capacity(BMP_HEADER + n * n * 4 + mask_stride * n);
    out.extend_from_slice(&(BMP_HEADER as u32).to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB — uncompressed
    out.extend_from_slice(&0u32.to_le_bytes()); // image size, optional for BI_RGB
    out.extend_from_slice(&0i32.to_le_bytes()); // x pixels per metre
    out.extend_from_slice(&0i32.to_le_bytes()); // y pixels per metre
    out.extend_from_slice(&0u32.to_le_bytes()); // palette entries used
    out.extend_from_slice(&0u32.to_le_bytes()); // palette entries required

    for y in (0..n).rev() {
        for x in 0..n {
            let i = (y * n + x) * 4;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }

    // A set bit means "let the background through".
    for y in (0..n).rev() {
        let mut row = vec![0u8; mask_stride];
        for x in 0..n {
            if rgba[(y * n + x) * 4 + 3] == 0 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&row);
    }
    out
}

#[cfg(test)]
mod tests;
