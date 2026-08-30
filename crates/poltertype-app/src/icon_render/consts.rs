//! Tray-icon geometry and fixed colors.

pub(crate) const W: u32 = 16;

pub(crate) const H: u32 = 16;

pub(crate) const PAUSED_BG: [u8; 4] = [0x6E, 0x6E, 0x6E, 0xFF];

/// The `mono` badge: one slate for every layout (issue #50).
///
/// Dark and desaturated so it sits quietly in a panel of either
/// polarity, but kept well away from [`PAUSED_BG`] — with the hue gone,
/// the only thing left saying "paused" would be three pixels of bar.
pub(crate) const MONO_BG: [u8; 4] = [0x34, 0x49, 0x5E, 0xFF];
