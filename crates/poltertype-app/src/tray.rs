//! Tray icon state rendering.

use poltertype_types::LayoutId;
use tracing::warn;
use tray_icon::TrayIcon;
use tray_icon::menu::MenuItem;

use crate::consts::*;
use crate::icon_render;
use crate::types::*;

pub(crate) fn tooltip_for(layout: Option<&LayoutId>, paused: bool, input_alert: bool) -> String {
    let base = match (layout, paused) {
        (Some(l), false) => format!("{APP_NAME} — {l}"),
        (Some(l), true) => format!("{APP_NAME} — {l} (paused)"),
        (None, false) => APP_NAME.to_owned(),
        (None, true) => format!("{APP_NAME} (paused)"),
    };
    if input_alert {
        format!("{base} — ⚠ no keyboard access, see Setup Guide")
    } else {
        base
    }
}

/// Redraw icon + tooltip + the pause menu-item text from the current
/// `TrayState`. Cheap (no allocation in the icon-rendering path beyond
/// a 16x16 RGBA buffer); safe to call on every state change.
pub(crate) fn refresh_tray(tray: &TrayIcon, item_pause: &MenuItem, state: &TrayState) {
    let icon_result = match state.layout.as_ref() {
        Some(l) => icon_render::for_layout(l, state.paused),
        None => icon_render::unknown(),
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
    ))) {
        warn!(?e, "could not update tray tooltip");
    }
    item_pause.set_text(if state.paused {
        "▶ Resume auto-switch"
    } else {
        "⏸ Pause auto-switch"
    });
}
