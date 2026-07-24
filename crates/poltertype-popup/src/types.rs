//! Plain data crossing the app ↔ popup boundary.

use std::time::Duration;

use crate::enums::PopupAnchor;

/// One clickable row of the tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupEntry {
    /// Replacement text, exactly as it would be typed — or the label
    /// of an action row (`is_action`).
    pub text: String,
    /// Short badge for entries that also switch the keyboard layout
    /// (e.g. `"UK"`) — rendered right-aligned and muted. `None` for
    /// plain spelling suggestions.
    pub badge: Option<String>,
    /// An action row ("Add to dictionary") rather than a replacement:
    /// rendered set-apart — divider above, accent-coloured label.
    pub is_action: bool,
}

/// Everything the popup needs to show one offer.
#[derive(Debug, Clone)]
pub struct PopupModel {
    /// Engine generation stamp; echoed back in
    /// [`crate::PopupUiEvent`] so stale interactions are ignorable.
    pub generation: u64,
    /// The mistyped word, shown struck-through in the header.
    pub original: String,
    /// Rows, in rank order. The engine caps these at 9 (digit keys).
    pub entries: Vec<PopupEntry>,
    /// Footer hint for the accept chord (e.g. `"Ctrl+Shift"`).
    /// `None` = keyboard accept disabled → click-only footer.
    pub accept_hint: Option<String>,
    /// How long to stay on screen before self-hiding with
    /// [`crate::PopupUiEvent::TimedOut`].
    pub timeout: Duration,
    /// Where to appear.
    pub anchor: PopupAnchor,
}
