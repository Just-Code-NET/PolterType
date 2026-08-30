//! Tray-icon geometry and fixed colors.

/// The design grid everything below is drawn on. The glyph font, the
/// pause bars and the waiting badge are all placed in these units.
pub(crate) const UNITS: u32 = 16;

/// How many real pixels one design unit becomes.
///
/// The icon used to *be* 16×16, which is what a panel at any scale
/// above 1 then had to enlarge — and an enlarged bitmap is the fuzzy,
/// pixelated badge issue #54 photographed next to its neighbours.
/// Handing the host a larger icon lets it scale *down* instead, which
/// every toolkit does with filtering. Integer, so a design unit stays a
/// whole number of pixels and the letters keep their edges.
pub(crate) const SCALE: u32 = 4;

pub(crate) const W: u32 = UNITS * SCALE;

pub(crate) const H: u32 = UNITS * SCALE;

pub(crate) const PAUSED_BG: [u8; 4] = [0x6E, 0x6E, 0x6E, 0xFF];

/// Nothing at all behind the `mono` letters.
///
/// A panel's other icons are flat monochrome glyphs on the panel
/// itself; a filled tile is what made ours the one foreign object in
/// the row (issue #54). `mono` is now the letters and no more.
pub(crate) const TRANSPARENT: [u8; 4] = [0x00, 0x00, 0x00, 0x00];

/// The `mono` letters, for a panel that is dark and one that is light.
pub(crate) const MONO_ON_DARK: [u8; 4] = [0xEC, 0xEF, 0xF4, 0xFF];
pub(crate) const MONO_ON_LIGHT: [u8; 4] = [0x1C, 0x22, 0x2B, 0xFF];

/// Drawn one unit around every `mono` letter, in the other polarity.
///
/// The panel's own colour is a guess — a probe of the *desktop's*
/// preference, which a panel is free to disagree with. The halo is what
/// makes a wrong guess cost legibility rather than the whole icon: with
/// it, light letters on a light panel are still an outlined shape.
pub(crate) const MONO_HALO_ON_DARK: [u8; 4] = [0x00, 0x00, 0x00, 0x8C];
pub(crate) const MONO_HALO_ON_LIGHT: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x8C];
