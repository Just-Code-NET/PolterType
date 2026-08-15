//! Palette, geometry and output sizes of the PolterType app icon.
//!
//! Every length below is in **design units** on a 64×64 canvas — the
//! same `viewBox` the site's `poltertype-web/public/favicon.svg` is
//! authored in, so the numbers can be read straight off that file and
//! the two marks stay identical. `render` scales them to the requested
//! output size.

use crate::RoundRect;

/// Side of the design canvas, in design units.
pub(crate) const UNITS: f32 = 64.0;

// ── output ────────────────────────────────────────────────────────────

/// Every size baked into the `.ico`, because Windows picks a different
/// one for the taskbar, the Start menu, Explorer's views and Alt-Tab,
/// and scales the nearest match when the exact size is absent — which
/// is the difference between a crisp icon and a smeared one.
///
/// Rendered directly at each size rather than downsampled from one
/// large image: the sampler already anti-aliases by coverage, and a
/// box filter over a 256 px master throws away exactly the contrast
/// the 16 px entry needs.
pub const ICO_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

/// Sizes at or above this go into the `.ico` PNG-compressed; smaller
/// ones as raw DIBs. The split is the shell's own convention — a
/// 256×256 BMP entry alone is 264 KB, and everything that reads a
/// 256 px icon at all is new enough to decode PNG.
pub(crate) const ICO_PNG_FROM: u32 = 256;

/// Floor for [`crate::render_png`]. Below this the smile's stroke is
/// well under a pixel and the mark stops being itself; the `.ico`'s
/// 16 px entry is deliberate and goes through a different door.
pub const MIN_PNG_SIZE: u32 = 32;

// ── palette ───────────────────────────────────────────────────────────

/// Indigo-600: the keycap's side wall and its recessed face.
pub(crate) const KEY_SIDE: [u8; 4] = [0x4F, 0x46, 0xE5, 0xFF];

/// A lighter indigo for the keycap's top surface — the 4-unit sliver of
/// it left uncovered by the well reads as a bevel.
pub(crate) const KEY_FACE: [u8; 4] = [0x6D, 0x65, 0xF2, 0xFF];

/// Off-white ghost body. Not pure white: keeps a hair of separation
/// from a white desktop background.
pub(crate) const GHOST: [u8; 4] = [0xF7, 0xF6, 0xFD, 0xFF];

/// Near-black indigo for eyes and smile.
pub(crate) const INK: [u8; 4] = [0x17, 0x14, 0x3A, 0xFF];

pub(crate) const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

// ── keycap ────────────────────────────────────────────────────────────

/// The key's side wall: same plate as the face, dropped 4 units. Only
/// its bottom edge stays visible.
pub(crate) const KEY_SIDE_RECT: RoundRect = RoundRect {
    x: 2.0,
    y: 6.0,
    w: 60.0,
    h: 54.0,
    r: 12.0,
};

/// The key's top surface.
pub(crate) const KEY_FACE_RECT: RoundRect = RoundRect {
    x: 2.0,
    y: 2.0,
    w: 60.0,
    h: 54.0,
    r: 12.0,
};

/// The recessed well the ghost sits in, inset 4 units into the face.
pub(crate) const KEY_WELL_RECT: RoundRect = RoundRect {
    x: 6.0,
    y: 6.0,
    w: 52.0,
    h: 46.0,
    r: 9.0,
};

// ── ghost ─────────────────────────────────────────────────────────────

/// Left and right edge of the ghost's body; the sides are vertical
/// between the dome and the hem.
pub(crate) const GHOST_LEFT: f32 = 17.0;
pub(crate) const GHOST_RIGHT: f32 = 47.0;

/// Apex of the dome, and the height at which it meets the vertical
/// sides.
pub(crate) const GHOST_TOP: f32 = 14.0;
pub(crate) const GHOST_SHOULDER: f32 = 30.2;

/// Superellipse exponent for the dome. A plain ellipse (2.0) draws a
/// crown too narrow for the mark; 2.13 is the exponent that puts the
/// curve through the quarter- and mid-points of the SVG's cubic, which
/// broadens the top and tightens the shoulders.
pub(crate) const GHOST_DOME_EXP: f32 = 2.13;

/// Where the straight sides end and the skirt begins. The body
/// rectangle stops here and `GHOST_HEM` takes over.
pub(crate) const GHOST_HEM_TOP: f32 = 43.0;

/// The skirt, as a closed outline: four lobes separated by three sharp
/// notches. These are the SVG path's own vertices, with its curved
/// segments flattened into three points each — the lobes need the
/// roundness, while the notches stay the hard corners they are in the
/// original. Wound left to right along the bottom, closing across the
/// top of the band.
pub(crate) const GHOST_HEM: [(f32, f32); 25] = [
    (GHOST_LEFT, GHOST_HEM_TOP),
    (17.0, 44.0),
    (17.33, 45.12),
    (18.18, 45.75),
    (19.28, 45.86),
    (20.4, 45.4),
    (23.2, 43.4), // notch
    (26.6, 46.0),
    (27.32, 46.39),
    (28.1, 46.53),
    (28.88, 46.39),
    (29.6, 46.0),
    (32.0, 44.0), // notch
    (34.4, 46.0),
    (35.12, 46.39),
    (35.9, 46.53),
    (36.68, 46.39),
    (37.4, 46.0),
    (40.8, 43.4), // notch
    (43.6, 45.4),
    (44.72, 45.86),
    (45.82, 45.75),
    (46.67, 45.12),
    (47.0, 44.0),
    (GHOST_RIGHT, GHOST_HEM_TOP),
];

pub(crate) const EYE_R: f32 = 2.6;
pub(crate) const EYE_Y: f32 = 30.0;
pub(crate) const EYE_LEFT_X: f32 = 26.5;
pub(crate) const EYE_RIGHT_X: f32 = 37.5;

/// The smile's centre line: the SVG's cubic sampled at eighths.
/// Stroking it as a polyline gets the round caps and joins for free —
/// every point within half a stroke width of the line is ink.
pub(crate) const SMILE_PATH: [(f32, f32); 9] = [
    (29.5, 36.5),
    (30.109, 36.927),
    (30.730, 37.231),
    (31.363, 37.414),
    (32.0, 37.475),
    (32.637, 37.414),
    (33.270, 37.231),
    (33.891, 36.927),
    (34.5, 36.5),
];

/// Half of the SVG's `stroke-width="2"`.
pub(crate) const SMILE_HALF_STROKE: f32 = 1.0;

/// `SMILE_PATH` grown by the stroke, as `(x0, y0, x1, y1)`. `in_smile`
/// runs for every sample in the image, so it rejects on this box before
/// measuring distance to anything.
pub(crate) const SMILE_BOX: (f32, f32, f32, f32) = (28.5, 35.5, 35.5, 38.5);
