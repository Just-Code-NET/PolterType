//! Threads that bridge engine/OS events into the tao event loop.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use poltertype_core::engine::{EngineCommand, SwitcherEvent};
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
            // No typed text here: this fires at INFO, the default
            // level, so it lands in a release build's on-disk log. The
            // layout transition plus the already-redacted reason is the
            // whole diagnostic story.
            info!(%from_layout, %to_layout, %reason, "correction applied");
            if settings.snapshot().general.show_notifications {
                spawn_layout_change_notification(layouts, &to_layout);
            }
        }
        SwitcherEvent::PausedChanged(paused) => {
            info!(paused, "engine paused state changed");
            state.paused = paused;
            refresh_tray(tray, item_pause, state);
            // Remembered on disk so the app comes back the way it was
            // left (issue #46). Guarded, because this event also
            // arrives when the file is what asked for the change.
            if settings.snapshot().general.paused != paused
                && let Err(e) = settings.update(|s| s.general.paused = paused)
            {
                warn!(?e, "could not remember the pause state");
            }
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
        | SwitcherEvent::AddToDictionary { .. }
        | SwitcherEvent::DictionaryOfferMissed { .. } => {}
    }
}

/// macOS: register who is sending, once, before the first notification.
///
/// `mac-notification-sys` needs a sender bundle id. When nobody set
/// one, its `ensure_application_set` asks LaunchServices for an app
/// literally named `use_default` (lib.rs:116 in 0.6.12) — and macOS
/// answers an unresolvable name with a modal **"Where is use_default?"
/// application chooser** over whatever the user was doing. Observed
/// live; and because the updater's error toasts bypass the
/// `show_notifications` gate, the dialog can appear even with
/// notifications switched off in the config.
///
/// `false` means there is no bundle to speak as — a bare binary run
/// from `cargo run` — and the caller skips its toast: a missing
/// notification is cheaper than that dialog.
#[cfg(target_os = "macos")]
fn notification_sender_ready() -> bool {
    use std::sync::OnceLock;
    static READY: OnceLock<bool> = OnceLock::new();
    *READY.get_or_init(|| {
        let Some(bundle_id) = main_bundle_identifier() else {
            info!("not running from an .app bundle; system notifications stay off");
            return false;
        };
        match notify_rust::set_application(&bundle_id) {
            Ok(()) => {
                info!(bundle = %bundle_id, "registered as notification sender");
                true
            }
            Err(e) => {
                warn!(?e, bundle = %bundle_id, "could not register as notification sender");
                false
            }
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn notification_sender_ready() -> bool {
    true
}

/// Our own `CFBundleIdentifier`, or `None` outside an `.app` bundle.
#[cfg(target_os = "macos")]
fn main_bundle_identifier() -> Option<String> {
    use core_foundation::string::CFString;
    let dict = core_foundation::bundle::CFBundle::main_bundle().info_dictionary();
    let key = CFString::from_static_string("CFBundleIdentifier");
    dict.find(&key)
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string())
}

/// Show a 2-second toast that the engine auto-switched layout.
///
/// On a worker thread because `notify-rust`'s `show()` is synchronous:
/// nothing cosmetic should add latency to the tray's event loop. Failures
/// are logged and swallowed — a missing notification daemon must not
/// propagate up.
pub(crate) fn spawn_layout_change_notification(layouts: &Arc<LayoutDb>, to_layout: &LayoutId) {
    if !notification_sender_ready() {
        return;
    }
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
                .icon(poltertype_shell::DESKTOP_ID)
                .timeout(notify_rust::Timeout::Milliseconds(2000));
            // `icon` names a `hicolor` theme entry, written at startup
            // by `poltertype_shell::install_desktop_entry`. Set
            // unconditionally: macOS has no such concept and ignores it.
            if let Err(e) = n.show() {
                warn!(?e, layout = %to_owned, "could not show layout-change notification");
            }
        })
        .ok();
}

/// "Added <word> to your Ukrainian dictionary."
///
/// Fires only for the implicit route into the dictionary — undoing a
/// correction — and only with notifications on. The tooltip's own "Add
/// to dictionary" row stays silent: a word that joined as a side effect
/// of a different gesture is the one that needs announcing.
///
/// The word is in the body on purpose. It never reaches the log —
/// `logsafe` sees to that — but "a word was added" without saying which
/// is not something anyone can act on.
pub(crate) fn spawn_dictionary_add_notification(
    layouts: &Arc<LayoutDb>,
    layout: &LayoutId,
    word: &str,
) {
    if !notification_sender_ready() {
        return;
    }
    let pretty = layouts
        .get(layout)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| layout.as_str().to_owned());
    let body = format!("Added “{word}” to your {pretty} dictionary — it won't be corrected again.");
    std::thread::Builder::new()
        .name("poltertype-notify-dict".into())
        .spawn(move || {
            let mut n = notify_rust::Notification::new();
            n.summary(APP_NAME)
                .body(&body)
                .appname(APP_NAME)
                .icon(poltertype_shell::DESKTOP_ID)
                .timeout(notify_rust::Timeout::Milliseconds(4000));
            if let Err(e) = n.show() {
                warn!(?e, "could not show dictionary-add notification");
            }
        })
        .ok();
}

/// Tell the user that something they were waiting on did not happen.
///
/// Deliberately **not** gated by `[general].show_notifications`: that
/// toggle governs the cosmetic "we switched your layout" chatter, while
/// this fires only when something is broken.
///
/// Longer timeout than the others, because this text has to be read.
pub(crate) fn spawn_error_notification(body: String) {
    if !notification_sender_ready() {
        return;
    }
    std::thread::Builder::new()
        .name("poltertype-notify-error".into())
        .spawn(move || {
            let mut n = notify_rust::Notification::new();
            n.summary(APP_NAME)
                .body(&body)
                .appname(APP_NAME)
                .icon(poltertype_shell::DESKTOP_ID)
                .timeout(notify_rust::Timeout::Milliseconds(8000));
            if let Err(e) = n.show() {
                warn!(?e, %body, "could not show error notification");
            }
        })
        .ok();
}

/// "PolterType 0.4.0 is ready — it will install when you restart."
///
/// Not gated by `[general].show_notifications`, for the same reason
/// [`spawn_error_notification`] is not. Fires at most once per released
/// version, and it is the only thing that tells a user who never opens
/// the tray menu that an update is waiting.
pub(crate) fn spawn_update_notification(version: &str) {
    if !notification_sender_ready() {
        return;
    }
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
                .icon(poltertype_shell::DESKTOP_ID)
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
            // Which chords are physically down, so a held one is one
            // press. The OS repeats `Pressed` while a chord is held,
            // and switch-last now acts on every fire it is given (the
            // engine's stash is no longer self-consuming — issue #37).
            //
            // Time-limited on purpose: if some platform ever delivers a
            // `Pressed` without its `Released`, this degrades to one
            // fire per second rather than a hotkey that works once and
            // then never again.
            let mut down: HashMap<u32, Instant> = HashMap::new();
            while let Ok(ev) = rx.recv() {
                // `global-hotkey` 0.6+ emits both `Pressed` and
                // `Released` for one chord. Forwarding both ran the
                // pause toggle twice per keypress, so pause only held
                // while the chord was physically down.
                if ev.state != HotKeyState::Pressed {
                    down.remove(&ev.id);
                    continue;
                }
                let now = Instant::now();
                match down.insert(ev.id, now) {
                    Some(since) if now.duration_since(since) < STUCK_HOTKEY_TIMEOUT => continue,
                    _ => {}
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

/// Poll the OS for the current layout every ~250 ms and emit
/// `LayoutChanged` when it differs from last time. The engine emits that
/// event only for its own switches, so this is what catches user-driven
/// ones (Win+Space, language bar, ibus…) and keeps the tray icon in sync.
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

/// What `config.toml` looks like from the outside. Compared as a whole,
/// including the `None` a replace-by-rename shows for an instant: a
/// spurious wake-up costs one re-read that reports no change.
fn config_stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// Notice a `config.toml` written by anybody else, and put it in force.
///
/// The running app used to re-read the file at exactly one moment: when
/// the Settings window it had spawned exited. So a hotkey rebound and
/// *saved* with that window still open did nothing, and a chord written
/// into the file by hand did nothing either — both until the app was
/// restarted, which is how it was reported (issue #45).
///
/// Only edits from outside get this far. `SettingsStore::update` keeps
/// the store's own snapshot in step with what it writes, so the tray's
/// own pause bookkeeping re-reads as no change and stops here.
///
/// Started with the rest of the event loop, which means the first
/// seconds after launch are unwatched — and on a session with no tray
/// host, building that loop initialises GTK and takes tens of seconds.
/// The chords themselves are armed before it for exactly that reason.
pub(crate) fn spawn_config_watcher(
    proxy: EventLoopProxy<UserEvent>,
    settings: Arc<SettingsStore>,
    engine_tx: crossbeam_channel::Sender<EngineCommand>,
) -> Result<()> {
    let path = settings.path().to_owned();
    std::thread::Builder::new()
        .name("poltertype-config-watcher".into())
        .spawn(move || {
            debug!(?path, "config watcher thread started");
            let mut last = config_stamp(&path);
            loop {
                std::thread::sleep(CONFIG_WATCH_INTERVAL);
                let now = config_stamp(&path);
                if now == last {
                    continue;
                }
                last = now;
                match settings.reload() {
                    Ok(false) => {}
                    Ok(true) => {
                        info!(?path, "config.toml changed on disk; applying it");
                        let paused = settings.snapshot().general.paused;
                        if engine_tx.send(EngineCommand::SettingsReloaded).is_err()
                            || engine_tx.send(EngineCommand::SetPaused(paused)).is_err()
                        {
                            break;
                        }
                        // The chords are re-applied in the event loop:
                        // the OS-level grabs live there and are not
                        // `Send`.
                        if proxy.send_event(UserEvent::SettingsChanged).is_err() {
                            break;
                        }
                    }
                    // Half-written, or edited into something that does
                    // not parse. Keep what is in force and wait for the
                    // next write rather than dropping the user's
                    // settings for a file they are still typing into.
                    Err(e) => warn!(
                        ?e,
                        "config.toml changed on disk but could not be read; keeping the settings in force"
                    ),
                }
            }
        })
        .context("spawn config watcher thread")?;
    Ok(())
}

#[cfg(test)]
mod tests;
