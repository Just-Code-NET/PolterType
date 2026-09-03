//! Layout metrics and colours for [`crate::renderer::Renderer`]. Sizes
//! are logical pixels (multiplied by the device scale at draw time).

/// 8-bit, non-premultiplied.
pub(crate) type Rgba = (u8, u8, u8, u8);

pub(crate) const MAX_W: f32 = 340.0;
pub(crate) const MIN_W: f32 = 200.0;
pub(crate) const PAD: f32 = 12.0;
pub(crate) const ROW_H: f32 = 30.0;
pub(crate) const ROW_GAP: f32 = 2.0;
pub(crate) const HEADER_H: f32 = 22.0;
pub(crate) const FOOTER_H: f32 = 18.0;
pub(crate) const PANEL_RADIUS: f32 = 10.0;
pub(crate) const BADGE_SIZE: f32 = 18.0;
pub(crate) const BADGE_RADIUS: f32 = 5.0;
pub(crate) const BADGE_GAP: f32 = 10.0;
pub(crate) const HOVER_RADIUS: f32 = 6.0;
pub(crate) const TAG_RADIUS: f32 = 4.0;
pub(crate) const TAG_PAD_X: f32 = 4.0;
pub(crate) const TAG_PAD_Y: f32 = 2.0;
pub(crate) const TAG_GAP: f32 = 8.0;

pub(crate) const HEADER_FONT: f32 = 13.0;
pub(crate) const ROW_FONT: f32 = 15.0;
pub(crate) const BADGE_FONT: f32 = 12.0;
pub(crate) const TAG_FONT: f32 = 11.0;
pub(crate) const FOOTER_FONT: f32 = 11.0;
// Comfortable single-line box; boxes are centered per element anyway.
pub(crate) const LINE_HEIGHT_FACTOR: f32 = 1.2;

pub(crate) const PANEL_BG: Rgba = (0x16, 0x16, 0x1E, 0xF2);
pub(crate) const PANEL_BORDER: Rgba = (0xFF, 0xFF, 0xFF, 0x24);
pub(crate) const HEADER_FG: Rgba = (0x9A, 0x9A, 0xB0, 0xFF);
pub(crate) const ROW_FG: Rgba = (0xEC, 0xEC, 0xF4, 0xFF);
/// Action rows ("Add to dictionary") — brand accent, set apart from
/// the plain replacement rows.
pub(crate) const ACTION_FG: Rgba = (0xA7, 0x8B, 0xFA, 0xFF);
/// Hairline divider drawn above the first action row.
pub(crate) const DIVIDER: Rgba = (0xFF, 0xFF, 0xFF, 0x1A);
pub(crate) const BADGE_BG: Rgba = (0x8B, 0x5C, 0xF6, 0xFF);
pub(crate) const BADGE_BG_HOVER: Rgba = (0xA7, 0x8B, 0xFA, 0xFF);
pub(crate) const BADGE_FG: Rgba = (0xFF, 0xFF, 0xFF, 0xFF);
pub(crate) const TAG_FG: Rgba = (0x8B, 0x8B, 0x9E, 0xFF);
pub(crate) const TAG_BG: Rgba = (0xFF, 0xFF, 0xFF, 0x14);
pub(crate) const HOVER_BG: Rgba = (0xFF, 0xFF, 0xFF, 0x12);
pub(crate) const FOOTER_FG: Rgba = (0x70, 0x70, 0x8A, 0xFF);

pub(crate) const ELLIPSIS: char = '…';
