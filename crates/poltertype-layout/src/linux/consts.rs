//! Environment variables the Linux backend reads.

/// Pins one backend instead of probing: `cinnamon`, `ibus`, `gnome`,
/// `kde`, `hyprland`, `fcitx`, `x11`, or `auto`. The first thing to ask
/// a bug reporter to try.
///
/// An unknown name, or a backend that cannot initialise here, is an
/// error rather than a quiet fall-through to the probe: "we picked a
/// different one and said nothing" is the exact failure this variable
/// exists to diagnose.
pub const BACKEND_ENV: &str = "POLTERTYPE_LAYOUT_BACKEND";
