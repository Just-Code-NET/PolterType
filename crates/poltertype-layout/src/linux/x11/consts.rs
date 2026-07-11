//! X11 switcher constants.

pub const BACKEND_NAME: &str = "linux-x11-xkb";

/// Root-window property where the X server records the XKB rules it was
/// configured with. NUL-separated: `rules\0model\0layout\0variant\0options`.
/// The `layout` field is the comma-separated layout list — the same
/// thing `setxkbmap -query` prints.
pub(crate) const RULES_NAMES_PROPERTY: &str = "_XKB_RULES_NAMES";

/// Index of the `layout` field inside `_XKB_RULES_NAMES`.
pub(crate) const LAYOUT_FIELD: usize = 2;

/// Bytes to request from the property. The layout list is a handful of
/// two-letter codes; 1 KiB is generous even for a keymap with every
/// group filled and a long options string.
pub(crate) const PROPERTY_LEN: u32 = 1024;

/// XKB tops out at four groups. Anything beyond this can't be locked,
/// so we refuse it rather than silently wrapping to another layout.
pub(crate) const MAX_GROUPS: usize = 4;
