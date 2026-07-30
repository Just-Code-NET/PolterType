//! Threads that bridge engine/OS events into the tao event loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use poltertype_core::engine::SwitcherEvent;
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::SettingsStore;
use poltertype_types::LayoutId;
use tao::event_loop::EventLoopProxy;
use tracing::{debug, info, warn};
use tray_icon::TrayIcon;
use tray_icon::menu::{MenuEvent, MenuItem};

use crate::consts::*;
use crate::enums::*;
use crate::tray::*;
use crate::types::*;

pub(crate) fn handle_engine_event(
    ev: SwitcherEvent,
    tray: &TrayIcon,
    item_pause: &MenuItem,
    state: &mut TrayState,
    settings: &Arc<SettingsStore>,
    layouts: &Arc<LayoutDb>,
) {
    match ev {
        SwitcherEvent::Corrected {
            from_layout,
            to_layout,
            original_text: _,
            corrected_text: _,
            reason,
        } => {
            // The typed text stays out of this line deliberately —
            // this fires at INFO, which is the default log level, so
            // anything here lands in the on-disk log of a release
            // build. The layout transition plus the (already
            // redacted) reason is the whole diagnostic story; the
            // words themselves are only visible via
            // `poltertype_types::logsafe` in an opted-in debug build.
            info!(%from_layout, %to_layout, %reason, "correction applied");
            // System notification — the user explicitly opted into
            // these via `[general].show_notifications`. The body
            // shows only the layout transition, which is the useful
            // "what just happened" signal.
            if settings.snapshot().general.show_notifications {
                spawn_layout_change_notification(layouts, &to_layout);
            }
        }
        SwitcherEvent::PausedChanged(paused) => {
            info!(paused, "engine paused state changed");
            state.paused = paused;
            refresh_tray(tray, item_pause, state);
        }
        SwitcherEvent::LayoutChanged(id) => {
            debug!(layout = %id, "layout changed");
            state.layout = Some(id);
            refresh_tray(tray, item_pause, state);
        }
        SwitcherEvent::KeptCurrent { reason } => {
            debug!(%reason, "decision: keep current");
        }
        // Handled in the event loop before delegation here (they need
        // the popup handle / dict handle / focus tracker, which are
        // loop-local).
        SwitcherEvent::SuggestionsReady { .. }
        | SwitcherEvent::SuggestionsDismissed { .. }
        | SwitcherEvent::SuggestionApplied { .. }
        | SwitcherEvent::AddToDictionary { .. } => {}
    }
}

/// Show a 2-second toast / notification that the engine just
/// auto-switched to a new layout. Spawned on a worker thread because
/// `notify-rust::Notification::show()` is synchronous and the time it
/// takes varies per platform (DBus round-trip on Linux, NSUserNotification
/// on macOS, Toast XML on Windows) — we don't want to add even a few ms
/// to the tray's event-loop latency for a cosmetic side effect.
///
/// Failures are logged at warn level and swallowed: a missing notification
/// daemon (Linux dev container, macOS sandbox quirks, Windows Focus
/// Assist suppressing toasts) shouldn't propagate up to the tray. The
/// auto-switch itself already happened — the notification is just the
/// optional UX sugar layer on top.
pub(crate) fn spawn_layout_change_notification(layouts: &Arc<LayoutDb>, to_layout: &LayoutId) {
    // Resolve the layout's display `name` if we have a mapping for it
    // (`English (United States)` for `en-US`, etc.). Falls back to the
    // raw BCP-47 id otherwise — never a panic, never a stale string.
    let pretty = layouts
        .get(to_layout)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| to_layout.as_str().to_owned());

    let to_owned = to_layout.as_str().to_owned();
    std::thread::Builder::new()
        .name("poltertype-notify".into())
        .spawn(move || {
            let mut n = notify_rust::Notification::new();
            n.summary("PolterType")
                .body(&format!("Switched to {pretty}"))
                .appname(APP_NAME)
                .timeout(notify_rust::Timeout::Milliseconds(2000));
            // `icon` is best-effort — passing an OS-specific identifier
            // works on Linux/Windows when a matching theme icon exists,
            // and is silently ignored otherwise. We don't ship our own
            // installed icon yet, so leave it out and let the platform's
            // default app-notification glyph render.
            if let Err(e) = n.show() {
                warn!(?e, layout = %to_owned, "could not show layout-change notification");
            }
        })
        .ok();
}

/// Tell the user that a tray action they just clicked did not happen.
///
/// Deliberately NOT gated by `[general].show_notifications`: that
/// toggle governs the cosmetic "we switched your layout" chatter that
/// fires during normal use. This one fires only when a menu click
/// produced nothing — the user is sitting there waiting for a window
/// that is never going to appear, and a `warn!` line in a log file
/// they don't know about is not a user interface.
///
/// Same worker-thread + swallow-failures contract as
/// [`spawn_layout_change_notification`]; a longer timeout because this
/// text has to be read, not glanced at.
pub(crate) fn spawn_error_notification(body: String) {
    std::thread::Builder::new()
        .name("poltertype-notify-error".into())
        .spawn(move || {
            let mut n = notify_rust::Notification::new();
            n.summary(APP_NAME)
                .body(&body)
                .appname(APP_NAME)
                .timeout(notify_rust::Timeout::Milliseconds(8000));
            if let Err(e) = n.show() {
                warn!(?e, %body, "could not show error notification");
            }
        })
        .ok();
}

/// "PolterType 0.4.0 is ready — it will install when you restart."
///
/// Not gated by `[general].show_notifications`, for the same reason the
/// error notification isn't: that toggle governs the cosmetic
/// per-correction chatter. This fires at most once per released version,
/// and it is the only thing that tells a user who never opens the tray
/// menu that an update is waiting for them. A silent update that
/// installs on some future quit, with no announcement, is exactly the
/// behaviour people mean when they complain that software updates
/// itself behind their back.
///
/// Same worker-thread + swallow-failures contract as the others.
pub(crate) fn spawn_update_notification(version: &str) {
    let body = format!(
        "Version {version} is downloaded and ready.\n\
         It will be installed the next time you restart PolterType — \
         or click \"Restart to update\" in the tray menu."
    );
    std::thread::Builder::new()
        .name("poltertype-notify-update".into())
        .spawn(move || {
            let mut n = notify_rust::Notification::new();
            n.summary(APP_NAME)
                .body(&body)
                .appname(APP_NAME)
                .timeout(notify_rust::Timeout::Milliseconds(8000));
            if let Err(e) = n.show() {
                warn!(?e, "could not show the update-ready notification");
            }
        })
        .ok();
}

pub(crate) fn spawn_event_bridges(
    proxy: EventLoopProxy<UserEvent>,
    engine_rx: Receiver<SwitcherEvent>,
) -> Result<()> {
    let proxy_menu = proxy.clone();
    std::thread::Builder::new()
        .name("tray-menu-bridge".into())
        .spawn(move || {
            let rx = MenuEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if proxy_menu.send_event(UserEvent::Menu(ev.id)).is_err() {
                    break;
                }
            }
        })
        .context("spawn menu bridge thread")?;

    let proxy_hk = proxy.clone();
    std::thread::Builder::new()
        .name("hotkey-bridge".into())
        .spawn(move || {
            let rx = GlobalHotKeyEvent::receiver();
            while let Ok(ev) = rx.recv() {
                // global-hotkey 0.6+ emits BOTH `Pressed` and
                // `Released` events for the same chord. Forwarding
                // both meant the pause-toggle handler ran twice per
                // user keypress — net effect: pause flipped on
                // press, then immediately back on release, so the
                // user only saw "paused" while physically holding
                // the chord. Filter to Pressed only.
                if ev.state != HotKeyState::Pressed {
                    continue;
                }
                if proxy_hk.send_event(UserEvent::Hotkey(ev.id)).is_err() {
                    break;
                }
            }
        })
        .context("spawn hotkey bridge thread")?;

    std::thread::Builder::new()
        .name("engine-event-bridge".into())
        .spawn(move || {
            for ev in engine_rx.iter() {
                if proxy.send_event(UserEvent::Engine(ev)).is_err() {
                    break;
                }
            }
        })
        .context("spawn engine event bridge thread")?;

    Ok(())
}

/// Forward suggestion-tooltip interactions (clicks, timeouts) into
/// the tao loop — same shape as the engine-event bridge.
pub(crate) fn spawn_popup_bridge(
    proxy: EventLoopProxy<UserEvent>,
    popup_rx: Receiver<poltertype_popup::PopupUiEvent>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("popup-event-bridge".into())
        .spawn(move || {
            for ev in popup_rx.iter() {
                if proxy.send_event(UserEvent::Popup(ev)).is_err() {
                    break;
                }
            }
        })
        .context("spawn popup event bridge thread")?;
    Ok(())
}

/// Poll the OS for the current layout every ~250 ms; emit a
/// `LayoutChanged` event whenever the answer differs from last time.
/// Catches manual switches done outside our engine (language bar,
/// Win+Space, Alt+Shift, ibus / kde keyboard, …) so the tray icon
/// stays in sync.
pub(crate) fn spawn_layout_poller(
    switcher: Arc<dyn poltertype_layout::LayoutSwitcher>,
    out_tx: crossbeam_channel::Sender<SwitcherEvent>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("poltertype-layout-poller".into())
        .spawn(move || {
            debug!("layout poller thread started");
            let mut last: Option<LayoutId> = None;
            loop {
                match switcher.current() {
                    Ok(current) => {
                        if last.as_ref() != Some(&current) {
                            debug!(
                                from = ?last,
                                to = %current,
                                "layout poller saw external switch"
                            );
                            if out_tx
                                .send(SwitcherEvent::LayoutChanged(current.clone()))
                                .is_err()
                            {
                                break;
                            }
                            last = Some(current);
                        }
                    }
                    Err(e) => {
                        debug!(?e, "layout poller: current() failed");
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .context("spawn layout poller thread")?;
    Ok(())
}
