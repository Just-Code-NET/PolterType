//! Icon palette and wordmark glyph bitmaps.

pub(crate) const INDIGO: [u8; 4] = [0x4F, 0x46, 0xE5, 0xFF]; // tailwind indigo-600

pub(crate) const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];

// 5×7 bitmap font, packed left-aligned in the high 5 bits of each byte
// (rows top-to-bottom; the low 3 bits are unused padding). Underscores
// follow nibble boundaries to keep clippy's `unusual_byte_groupings`
// happy — read the high nibble + the top bit of the low nibble for the
// pixel pattern.
pub(crate) const GLYPH_K: [u8; 7] = [
    0b1000_1000,
    0b1001_0000,
    0b1010_0000,
    0b1100_0000,
    0b1010_0000,
    0b1001_0000,
    0b1000_1000,
];

pub(crate) const GLYPH_B: [u8; 7] = [
    0b1111_0000,
    0b1000_1000,
    0b1000_1000,
    0b1111_0000,
    0b1000_1000,
    0b1000_1000,
    0b1111_0000,
];
