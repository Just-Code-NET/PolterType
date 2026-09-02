//! Where the flag sits on the tray icon's design grid, and the two
//! colours half the drawings are made of.

use crate::icon_render::{SCALE, W};

/// The flag's box in real pixels: the icon's full width, and 12 of its
/// 16 design units tall, starting two units down.
///
/// 4:3 is no flag's own ratio, but it is *one* ratio for all of them.
/// Drawing each at its own would make Switzerland a third the size of
/// Denmark in the same panel, and a panel row reads as a row of
/// same-sized icons.
pub(crate) const FX: u32 = 0;
pub(crate) const FY: u32 = 2 * SCALE;
pub(crate) const FW: u32 = W;
pub(crate) const FH: u32 = 12 * SCALE;

/// The edge drawn around the flag, for a dark panel and a light one.
///
/// Half of these flags end in white and several end in black; without
/// an edge those sides dissolve into a panel of the same colour and
/// the flag loses its shape. One design unit thick, because a panel
/// shows the icon at a quarter of this size and anything thinner
/// averages away to nothing there.
pub(crate) const EDGE_ON_DARK: [u8; 4] = [0xEC, 0xEF, 0xF4, 0xB4];
pub(crate) const EDGE_ON_LIGHT: [u8; 4] = [0x1C, 0x22, 0x2B, 0xB4];
pub(crate) const EDGE: u32 = SCALE;

/// The drawing's own box, inside that edge. The flag is painted here
/// rather than under the edge, so a frame does not eat a band.
pub(crate) const IX: u32 = FX + EDGE;
pub(crate) const IY: u32 = FY + EDGE;
pub(crate) const IW: u32 = FW - 2 * EDGE;
pub(crate) const IH: u32 = FH - 2 * EDGE;

/// Width over height of the drawing, so a disc placed in its
/// fractions still comes out round.
pub(crate) const ASPECT: f32 = IW as f32 / IH as f32;

pub(crate) const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
pub(crate) const BLACK: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];

/// The Nordic cross. The upright sits left of centre; the half-widths
/// are chosen so both arms come out the same number of pixels wide on
/// a box that is not square.
pub(crate) const CROSS_U: f32 = 0.34;
pub(crate) const CROSS_HW: f32 = 0.075;
pub(crate) const CROSS_HH: f32 = 0.10;

/// Norway's and Iceland's second cross, as a fraction of the first.
pub(crate) const INNER_CROSS: f32 = 0.42;

/// A five-pointed star's inner radius over its outer one.
pub(crate) const STAR_WAIST: f32 = 0.382;

/// The greys a paused flag is flattened into.
///
/// Dimmed rather than merely drained: "inactive" has to be readable
/// at a glance, and a mid-grey Ukraine and a mid-grey Germany are the
/// same picture. The band is still wide enough to keep the bands, the
/// cross or the disc — which is what names the country once the
/// colour is gone — and dark enough for the white pause bars.
pub(crate) const GREY_FLOOR: u8 = 0x30;
pub(crate) const GREY_RANGE: u8 = 0x5C;
