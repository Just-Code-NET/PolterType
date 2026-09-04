//! Tray icon state rendering.

use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::TrayIconStyle;
use poltertype_types::LayoutId;
use poltertype_update::PendingUpdate;
use tracing::{debug, warn};
use tray_icon::TrayIcon;
use tray_icon::menu::{MenuId, MenuItem, Submenu};

use crate::consts::*;
use crate::icon_render;
use crate::plugins;
use crate::types::*;

pub(crate) fn tooltip_for(
    layout: Option<&LayoutId>,
    paused: bool,
    input_alert: bool,
    attention: u32,
) -> String {
    // The product name and the layout id are not words to translate;
    // everything the tooltip *says* around them is.
    let idle = tr("tray.tooltip_paused", "(paused)");
    let base = match (layout, paused) {
        (Some(l), false) => format!("{APP_NAME} — {l}"),
        (Some(l), true) => format!("{APP_NAME} — {l} {idle}"),
        (None, false) => APP_NAME.to_owned(),
        (None, true) => format!("{APP_NAME} {idle}"),
    };
    let base = if input_alert {
        format!(
            "{base} — {}",
            tr(
                "tray.tooltip_no_keyboard",
                "⚠ no keyboard access, see Setup Guide",
            )
        )
    } else {
        base
    };
    // The mark on the icon says *that* something is waiting; the tooltip
    // is the only place the count fits without opening anything.
    match attention {
        0 => base,
        1 => format!("{base} — {}", tr("tray.tooltip_draft", "1 draft waiting")),
        n => format!(
            "{base} — {}",
            tr_args(
                "tray.tooltip_drafts",
                "{} drafts waiting",
                &[&n.to_string()]
            )
        ),
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

/// Write every entry PolterType owns the words of, from the catalog
/// loaded right now.
///
/// Called once as the menu is built and again whenever the interface
/// language changes, which is why the entries are created empty: the
/// words exist here and nowhere else, so the menu on screen and the
/// menu after a language change cannot drift apart.
///
/// `hooks_missing` picks which failure the Setup entry names — fixed at
/// startup, since the only recovery is fixing permissions and
/// relaunching.
pub(crate) fn relabel_menu(menu: &TrayMenu, hooks_missing: bool, pending: Option<&PendingUpdate>) {
    if let Some(item) = &menu.setup {
        item.set_text(if hooks_missing {
            tr("tray.alert_hooks", "⚠ Keyboard hooks unavailable — Setup…")
        } else {
            tr(
                "tray.alert_switching",
                "⚠ Layout switching unavailable — Setup…",
            )
        });
    }
    menu.settings_ui.set_text(tr("tray.settings", "Settings…"));
    menu.settings_file
        .set_text(tr("tray.edit_config", "Edit config.toml…"));
    menu.logs
        .set_text(tr("tray.open_logs", "Open Logs Folder…"));
    menu.wordlists
        .set_text(tr("tray.open_wordlists", "Open User Wordlists Folder…"));
    menu.layouts
        .set_text(tr("tray.open_layouts", "Open User Layouts Folder…"));
    menu.reload
        .set_text(tr("tray.reload_settings", "Reload Settings"));
    menu.deferred
        .set_text(tr("tray.deferred", DEFERRED_MENU_LABEL));
    if let Some(item) = &menu.update {
        crate::updater::refresh_menu_item(item, pending);
    }
    menu.about.set_text(tr_args(
        "tray.about",
        "About {} v{}",
        &[APP_NAME, env!("CARGO_PKG_VERSION")],
    ));
    menu.quit.set_text(tr("tray.quit", "Quit"));
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
        tr("tray.resume", "▶ Resume auto-switch")
    } else {
        tr("tray.pause", "⏸ Pause auto-switch")
    }
}

/// Repopulate the "missed words" submenu from `deferred`, and record
/// which menu id stands for which word so a click can be resolved.
///
/// Rebuilt wholesale rather than patched: the list is at most eight
/// rows and changes only when a tooltip is missed or a word is taken,
/// so the simple thing is also the fast one.
pub(crate) fn rebuild_deferred_menu(
    submenu: &Submenu,
    deferred: &DeferredWords,
    rows: &mut Vec<(MenuId, LayoutId, String)>,
    layouts: &LayoutDb,
) {
    // Back to front: `remove_at` shifts everything after the index it
    // takes, so walking forwards would skip every other row and leave
    // stale ones behind — which then resolve to words already added.
    for i in (0..submenu.items().len()).rev() {
        let _ = submenu.remove_at(i);
    }
    rows.clear();
    if deferred.is_empty() {
        // A submenu that is empty *and* disabled is indistinguishable
        // from one that is broken: reported as "I click it and nothing
        // happens" (issue #38). One disabled row says which it is.
        let empty = MenuItem::new(tr("tray.deferred_empty", DEFERRED_MENU_EMPTY), false, None);
        if let Err(e) = submenu.append(&empty) {
            warn!(?e, "could not add the missed-word placeholder");
        }
        debug!("tray: missed-word list rebuilt rows=0");
        return;
    }
    for (layout, word) in deferred.iter() {
        // The layout is named because the same spelling can be a word
        // in one and gibberish in another, and the entry goes into one
        // wordlist, not both.
        let name = layouts
            .get(layout)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| layout.as_str().to_owned());
        let item = MenuItem::new(format!("{word}  ·  {name}"), true, None);
        rows.push((item.id().clone(), layout.clone(), word.clone()));
        if let Err(e) = submenu.append(&item) {
            warn!(?e, "could not add a missed word to the submenu");
        }
    }
    // Count only. The whole point of this list is that it holds text
    // the user typed, so it is the one thing that must never reach a
    // log — see `logsafe`.
    debug!(rows = rows.len(), "tray: missed-word list rebuilt");
}

/// Move the mark on the tray icon to match what the plug-ins are waiting
/// on, redrawing only when the number changed: the icon is rasterised
/// from scratch on every redraw.
pub(crate) fn sync_attention(
    tray: &TrayIcon,
    item_pause: &MenuItem,
    state: &mut TrayState,
    menu: &plugins::PluginMenu,
) {
    if state.attention == menu.attention() {
        return;
    }
    state.attention = menu.attention();
    refresh_tray(tray, item_pause, state);
}
