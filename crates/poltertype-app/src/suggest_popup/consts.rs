//! Tuning constants for suggestion-tooltip anchoring.

use std::time::Duration;

/// A caret sample older than this is distrusted: the user has since
/// focused an app that emits no a11y caret events, and the focused
/// window describes the present better than a caret from the past.
/// Generous enough to survive the word being typed (each keystroke
/// refreshes the sample in a11y-capable apps).
pub(super) const CARET_MAX_AGE: Duration = Duration::from_secs(5);
