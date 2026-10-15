//! Building one popup model from an engine offer, and showing it.

use std::sync::Arc;

use poltertype_core::engine::SuggestionAction;
use poltertype_core::settings::{SuggestionSettings, TooltipSpot};
use poltertype_input::FocusTracker;
use poltertype_popup::{PopupEntry, PopupModel, SuggestionPopup, WindowSpot};
use poltertype_types::LayoutId;
use tracing::debug;

use super::anchor::resolve_anchor;
use super::types::PopupOffer;

/// Build the popup model for one offer and show it. The anchor is
/// resolved *now*, at offer time — see [`resolve_anchor`] for the
/// chain — and the look is read from `prefs` at the same moment, so a
/// saved change applies to the next tooltip. Returns the model shown,
/// which the keyboard highlight re-shows with a row selected: same
/// anchor, same place.
pub(crate) fn show_suggestion_popup(
    popup: &dyn SuggestionPopup,
    focus_tracker: &Arc<dyn FocusTracker>,
    prefs: &SuggestionSettings,
    offer: PopupOffer,
) -> PopupModel {
    let PopupOffer {
        generation,
        original,
        entries,
        timeout,
        accept_modifiers,
        function_keys,
        arrows,
    } = offer;
    // `caret_anchor = false` means the window every time. On Linux the
    // tracker was already built without a caret source; elsewhere the
    // caret is read through ordinary APIs, so it is skipped here.
    let caret = prefs
        .caret_anchor
        .then(|| focus_tracker.caret_hint())
        .flatten();
    let anchor = resolve_anchor(
        focus_tracker.focused_window_geometry(),
        caret,
        window_spot(prefs.window_spot()),
        prefs.window_margin_clamped(),
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
    let model = PopupModel {
        generation,
        original,
        entries,
        accept_hint: (!accept_modifiers.is_empty()).then_some(accept_modifiers),
        timeout,
        anchor,
        scale: prefs.tooltip_scale(),
        function_keys,
        arrows,
        selected: None,
    };
    popup.show(model.clone());
    model
}

/// The settings' spelling of a grid cell → the popup crate's.
pub(super) fn window_spot(spot: TooltipSpot) -> WindowSpot {
    match spot {
        TooltipSpot::TopLeft => WindowSpot::TopLeft,
        TooltipSpot::TopCenter => WindowSpot::TopCenter,
        TooltipSpot::TopRight => WindowSpot::TopRight,
        TooltipSpot::MiddleLeft => WindowSpot::MiddleLeft,
        TooltipSpot::Center => WindowSpot::Center,
        TooltipSpot::MiddleRight => WindowSpot::MiddleRight,
        TooltipSpot::BottomLeft => WindowSpot::BottomLeft,
        TooltipSpot::BottomCenter => WindowSpot::BottomCenter,
        TooltipSpot::BottomRight => WindowSpot::BottomRight,
    }
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
