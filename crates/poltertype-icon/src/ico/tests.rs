//! `.ico` container tests.
//!
//! These read the bytes back the way Windows does rather than checking
//! that we wrote *something*: an icon with a plausible header and a
//! wrong offset table is a file the shell silently declines to draw,
//! which is indistinguishable from having no icon at all — the bug
//! this crate exists to fix.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// `(declared axis, offset, length)` for each directory entry.
fn entries(ico: &[u8]) -> Vec<(u8, usize, usize)> {
    (0..u16_at(ico, 4) as usize)
        .map(|i| {
            let at = DIR_HEADER + i * DIR_ENTRY;
            (
                ico[at],
                u32_at(ico, at + 12) as usize,
                u32_at(ico, at + 8) as usize,
            )
        })
        .collect()
}

#[test]
fn the_directory_names_every_size_once() {
    let ico = encode_ico().expect("encode the icon");

    assert_eq!(u16_at(&ico, 0), 0, "reserved word must be zero");
    assert_eq!(u16_at(&ico, 2), 1, "type 1 is an icon");
    assert_eq!(u16_at(&ico, 4) as usize, ICO_SIZES.len());

    // 256 is written as 0 — one byte cannot hold it.
    let declared: Vec<u32> = entries(&ico)
        .iter()
        .map(|&(axis, _, _)| if axis == 0 { 256 } else { u32::from(axis) })
        .collect();
    assert_eq!(declared, ICO_SIZES);
}

#[test]
fn every_entry_points_at_bytes_inside_the_file() {
    let ico = encode_ico().expect("encode the icon");
    let mut expected = DIR_HEADER + DIR_ENTRY * ICO_SIZES.len();

    for (axis, offset, len) in entries(&ico) {
        assert_eq!(offset, expected, "entry {axis} starts where the last ended");
        assert!(len > 0, "entry {axis} is empty");
        assert!(
            offset + len <= ico.len(),
            "entry {axis} runs {} bytes past the end",
            offset + len - ico.len()
        );
        expected += len;
    }
    assert_eq!(expected, ico.len(), "trailing bytes nobody points at");
}

#[test]
fn the_large_entry_is_a_png_and_the_small_ones_are_dibs() {
    let ico = encode_ico().expect("encode the icon");

    for (i, (_, offset, _)) in entries(&ico).iter().enumerate() {
        let size = ICO_SIZES[i];
        let head = &ico[*offset..*offset + 4];
        if size >= ICO_PNG_FROM {
            assert_eq!(head, PNG_MAGIC, "{size} px should be PNG-compressed");
        } else {
            assert_eq!(
                u32_at(&ico, *offset),
                BMP_HEADER as u32,
                "{size} px should open with a BITMAPINFOHEADER"
            );
        }
    }
}

#[test]
fn a_dib_declares_a_doubled_height_and_is_exactly_as_long_as_it_claims() {
    let size = 32u32;
    let bytes = dib(&rasterise(size), size);

    assert_eq!(u32_at(&bytes, 4), size, "width");
    assert_eq!(
        u32_at(&bytes, 8),
        size * 2,
        "height counts the colour rows and the mask together"
    );
    assert_eq!(u16_at(&bytes, 14), 32, "bits per pixel");

    let n = size as usize;
    let mask = n.div_ceil(32) * 4 * n;
    assert_eq!(bytes.len(), BMP_HEADER + n * n * 4 + mask);
}

#[test]
fn the_mask_marks_exactly_the_transparent_pixels() {
    let size = 32u32;
    let rgba = rasterise(size);
    let bytes = dib(&rgba, size);

    let n = size as usize;
    let clear = rgba.chunks_exact(4).filter(|px| px[3] == 0).count();
    assert!(
        clear > 0,
        "the mark has rounded corners; some pixels are clear"
    );

    let mask = &bytes[BMP_HEADER + n * n * 4..];
    let set: u32 = mask.iter().map(|b| b.count_ones()).sum();
    assert_eq!(
        set as usize, clear,
        "one set bit per fully transparent pixel"
    );
}

#[test]
fn colour_rows_run_bottom_up_in_bgra() {
    let size = 32u32;
    let rgba = rasterise(size);
    let bytes = dib(&rgba, size);

    // The first row written is the image's *last* row, and each pixel
    // is byte-swapped. Getting either wrong flips or recolours the
    // icon without changing its length, so nothing else would notice.
    let n = size as usize;
    let last_row = (n - 1) * n * 4;
    let first_written = BMP_HEADER;
    for x in 0..n {
        let src = &rgba[last_row + x * 4..last_row + x * 4 + 4];
        let out = &bytes[first_written + x * 4..first_written + x * 4 + 4];
        assert_eq!(out, [src[2], src[1], src[0], src[3]], "pixel {x}");
    }
}
