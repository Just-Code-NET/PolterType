//! Tuning constants for suggestion-tooltip anchoring.

use std::time::Duration;

/// A caret sample older than this is distrusted: the user has since
/// focused an app that emits no a11y caret events, and the focused
/// window describes the present better than a caret from the past.
/// Generous enough to survive the word being typed (each keystroke
/// refreshes the sample in a11y-capable apps).
pub(super) const CARET_MAX_AGE: Duration = Duration::from_secs(5);

/// How far the window size an app reports for itself may differ from
/// the compositor's rect before the two are taken to be different
/// windows. Every toolkit measured — GTK, Qt, Chromium, Gecko, both
/// natively and through XWayland — answered to the pixel, so this is
/// rounding slack rather than a real tolerance.
pub(super) const WINDOW_SIZE_SLACK: u32 = 4;
