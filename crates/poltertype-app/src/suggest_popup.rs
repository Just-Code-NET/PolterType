//! Glue between the engine's suggestion events and the tooltip
//! backend: model building and anchor resolution.

use std::sync::Arc;
use std::time::Duration;

use poltertype_core::engine::{SuggestionAction, SuggestionEntry};
use poltertype_input::FocusTracker;
use poltertype_popup::{PopupAnchor, PopupEntry, PopupModel, SuggestionPopup};
use poltertype_types::LayoutId;

/// A caret sample older than this is distrusted: the user has since
/// focused an app that emits no a11y caret events, and the pointer /
/// window anchors describe the present better than a caret from the
/// past. Generous enough to survive the word being typed (each
/// keystroke refreshes the sample in a11y-capable apps).
const CARET_MAX_AGE: Duration = Duration::from_secs(5);

/// Build the popup model for one offer and show it.
///
/// Anchor chain, best first — resolved *now*, at offer time:
///
/// 1. **AT-SPI caret** — the real text-insertion point, when the
///    focused app exposes it, the sample is fresh, and it lies inside
///    the focused window (a stale caret from a previous window must
///    not win).
/// 2. **Pointer inside the focused window** — after a click into the
///    text being edited the pointer hovers near the caret.
/// 3. **Focused window** — bottom-centre, the neighbourhood of chat
///    inputs and prompts.
/// 4. **Screen bottom** — nothing known (GNOME/KDE Wayland).
pub(crate) fn show_suggestion_popup(
    popup: &dyn SuggestionPopup,
    focus_tracker: &Arc<dyn FocusTracker>,
    generation: u64,
    original: String,
    entries: Vec<SuggestionEntry>,
    timeout: Duration,
    accept_modifiers: String,
) {
    let anchor = match focus_tracker.focused_window_geometry() {
        Some(g) => {
            let inside = |px: i32, py: i32| {
                px >= g.x && px < g.x + g.width as i32 && py >= g.y && py < g.y + g.height as i32
            };
            // The caret hint is window-relative (see `CaretHint`) —
            // compose with the live window rect, then sanity-check it
            // actually lands inside that rect (a nonsense answer from
            // a broken a11y bridge must not fling the tooltip away).
            let caret = focus_tracker
                .caret_hint()
                .filter(|c| c.age <= CARET_MAX_AGE)
                .map(|c| (g.x + c.x, g.y + c.y, c.height))
                .filter(|&(cx, cy, _)| inside(cx, cy));
            if let Some((cx, cy, height)) = caret {
                PopupAnchor::Point {
                    x: cx,
                    y: cy,
                    height,
                    output: g.output,
                    output_x: g.output_x,
                    output_y: g.output_y,
                }
            } else if let Some((px, py)) = focus_tracker
                .pointer_position()
                .filter(|&(px, py)| inside(px, py))
            {
                PopupAnchor::Point {
                    x: px,
                    y: py,
                    height: 0,
                    output: g.output,
                    output_x: g.output_x,
                    output_y: g.output_y,
                }
            } else {
                PopupAnchor::WindowRect {
                    x: g.x,
                    y: g.y,
                    width: g.width,
                    height: g.height,
                    output: g.output,
                    output_x: g.output_x,
                    output_y: g.output_y,
                }
            }
        }
        None => PopupAnchor::ScreenBottom { output: None },
    };
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
                text: "Add to dictionary".to_owned(),
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

/// Short badge for a cross-layout entry: the language subtag,
/// uppercased — `uk-UA` → `UK`, `en-US` → `EN`. Falls back to the
/// whole id for exotic single-part ids.
fn layout_badge(id: &LayoutId) -> String {
    id.as_str()
        .split('-')
        .next()
        .unwrap_or(id.as_str())
        .to_uppercase()
}
