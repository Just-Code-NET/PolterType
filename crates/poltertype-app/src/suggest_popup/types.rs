//! What the engine's offer hands the tooltip glue.

use std::time::Duration;

use poltertype_core::engine::SuggestionEntry;

/// One offer, as `SwitcherEvent::SuggestionsReady` carries it.
pub(crate) struct PopupOffer {
    pub generation: u64,
    pub original: String,
    pub entries: Vec<SuggestionEntry>,
    pub timeout: Duration,
    /// Digit-chord modifiers for the footer; empty when the chord is
    /// off.
    pub accept_modifiers: String,
    pub function_keys: bool,
    pub arrows: bool,
}
