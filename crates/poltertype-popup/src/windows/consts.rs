//! Tunables and Win32 spellings for the layered-window backend.

use std::time::Duration;

/// How often the popup thread wakes to pump messages and check its
/// deadline. Matches the X11 backend's tick — a tooltip is not
/// animated, so this only bounds how fast a click is noticed.
pub(super) const TICK: Duration = Duration::from_millis(16);

/// Window class name. Registered once per process; a second
/// registration of the same name is the harmless error we ignore.
pub(super) const CLASS_NAME: windows::core::PCWSTR = windows::core::w!("PolterTypeSuggestionPopup");

/// The DPI Windows reports for a 100% display. Scale is
/// `dpi / BASE_DPI`, which is what the renderer means by "scale".
pub(super) const BASE_DPI: u32 = 96;
