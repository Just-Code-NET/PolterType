//! Building one popup model from an engine offer, and showing it.

use std::sync::Arc;
use std::time::Duration;

use poltertype_core::engine::{SuggestionAction, SuggestionEntry};
use poltertype_input::FocusTracker;
use poltertype_popup::{PopupEntry, PopupModel, SuggestionPopup};
use poltertype_types::LayoutId;
use tracing::debug;

use super::anchor::resolve_anchor;

/// Build the popup model for one offer and show it. The anchor is
/// resolved *now*, at offer time — see [`resolve_anchor`] for the
/// chain.
pub(crate) fn show_suggestion_popup(
    popup: &dyn SuggestionPopup,
    focus_tracker: &Arc<dyn FocusTracker>,
    generation: u64,
    original: String,
    entries: Vec<SuggestionEntry>,
    timeout: Duration,
    accept_modifiers: String,
) {
    let anchor = resolve_anchor(
        focus_tracker.focused_window_geometry(),
        focus_tracker.caret_hint(),
    );
    debug!(?anchor, "suggestion popup anchor resolved");
    let entries = entries
        .into_iter()
        .map(|e| match e.action {
            SuggestionAction::Replace => PopupEntry {
                badge: e.switch_to.as_ref().map(layout_badge),
                text: e.text,
                is_action: false,
            },
            // The engine keeps the word in `e.text`; the tooltip
            // shows a label instead — the word is already in the
            // struck-through header right above.
            SuggestionAction::AddToDictionary => PopupEntry {
                badge: None,
                text: poltertype_core::i18n::tr("popup.add_to_dictionary", "Add to dictionary")
                    .to_owned(),
                is_action: true,
            },
        })
        .collect();
    popup.show(PopupModel {
        generation,
        original,
        entries,
        accept_hint: (!accept_modifiers.is_empty()).then_some(accept_modifiers),
        timeout,
        anchor,
    });
}

/// Badge for a cross-layout entry: the language subtag, uppercased
/// (`uk-UA` → `UK`); the whole id when it has no subtag.
fn layout_badge(id: &LayoutId) -> String {
    id.as_str()
        .split('-')
        .next()
        .unwrap_or(id.as_str())
        .to_uppercase()
}
