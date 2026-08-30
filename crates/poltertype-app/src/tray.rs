//! Tray icon state rendering.

use poltertype_core::settings::TrayIconStyle;
use poltertype_types::LayoutId;
use tracing::warn;
use tray_icon::TrayIcon;
use tray_icon::menu::MenuItem;

use crate::consts::*;
use crate::icon_render;
use crate::types::*;

pub(crate) fn tooltip_for(
    layout: Option<&LayoutId>,
    paused: bool,
    input_alert: bool,
    attention: u32,
) -> String {
    let base = match (layout, paused) {
        (Some(l), false) => format!("{APP_NAME} — {l}"),
        (Some(l), true) => format!("{APP_NAME} — {l} (paused)"),
        (None, false) => APP_NAME.to_owned(),
        (None, true) => format!("{APP_NAME} (paused)"),
    };
    let base = if input_alert {
        format!("{base} — ⚠ no keyboard access, see Setup Guide")
    } else {
        base
    };
    // The mark on the icon says *that* something is waiting; the tooltip
    // is the only place the count fits without opening anything.
    match attention {
        0 => base,
        1 => format!("{base} — 1 draft waiting"),
        n => format!("{base} — {n} drafts waiting"),
    }
}

/// Redraw icon, tooltip and the pause item's text from `TrayState`. The
/// icon is rasterised from scratch, so call this on a state change, not
/// on a tick.
pub(crate) fn refresh_tray(tray: &TrayIcon, item_pause: &MenuItem, state: &TrayState) {
    let waiting = state.attention > 0;
    let icon_result = match state.layout.as_ref() {
        Some(l) => icon_render::for_layout(l, state.paused, waiting, state.style, state.polarity),
        None => icon_render::unknown(waiting),
    };
    match icon_result {
        Ok(icon) => {
            if let Err(e) = tray.set_icon(Some(icon)) {
                warn!(?e, "could not update tray icon");
            }
        }
        Err(e) => warn!(?e, "could not render tray icon"),
    }
    if let Err(e) = tray.set_tooltip(Some(tooltip_for(
        state.layout.as_ref(),
        state.paused,
        state.input_alert,
        state.attention,
    ))) {
        warn!(?e, "could not update tray tooltip");
    }
    item_pause.set_text(pause_item_label(state.paused));
}

/// Show or hide the tray icon, per `[general].tray_icon`.
///
/// Hiding it takes the menu with it, and the menu is the whole of this
/// app's UI — what is left is `poltertype --settings`, which is what
/// issue #50 asked for. On Linux this asks the desktop for
/// `AppIndicatorStatus::Passive`, and a StatusNotifier host is allowed
/// to go on drawing a passive item: a request, not a guarantee.
pub(crate) fn apply_tray_visibility(tray: &TrayIcon, style: TrayIconStyle) {
    if let Err(e) = tray.set_visible(style != TrayIconStyle::Hidden) {
        warn!(?e, "could not change the tray icon's visibility");
    }
}

/// The pause item's own text. Shared with the menu's construction: the
/// app can now start paused, and a menu built from a fixed string would
/// offer to pause something that already is.
pub(crate) fn pause_item_label(paused: bool) -> &'static str {
    if paused {
        "▶ Resume auto-switch"
    } else {
        "⏸ Pause auto-switch"
    }
}
