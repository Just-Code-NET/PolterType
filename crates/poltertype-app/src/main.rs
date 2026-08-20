//! poltertype application entry point: wires the tray, global
//! keyboard listener, layout switcher and `SwitcherEngine` together,
//! registers the two built-in hotkeys, and spawns the focus-driven
//! wordlist-profile watcher.
//!
//! The Settings GUI is a separate process (`poltertype --settings`).
//! See `docs/ARCHITECTURE.md`.

// A tray-only app must not own a console: without this Windows links
// the binary as a CUI image and allocates a conhost the moment it is
// started by anything that is not already a console — which is every
// way a user launches it.
//
// Unconditional rather than `not(debug_assertions)`, so the shape we
// test is the shape we ship: the subsystem also decides whether a
// spawned plug-in inherits our console. Diagnostics do not depend on
// it — `init_tracing` writes to a file, and a GUI image still inherits
// standard handles. Ignored on every other platform.
#![windows_subsystem = "windows"]
#![forbid(unsafe_code)]

mod icon_render;
mod settings_ui;

mod bridges;
mod consts;
mod detectors;
mod enums;
mod hotkeys;
mod plugins;
mod settings_proc;
mod suggest_popup;
mod tray;
mod types;
mod updater;
mod user_dirs;

use crate::bridges::*;
use std::time::Instant;

use crate::consts::*;
use crate::detectors::*;
use crate::enums::*;
use crate::hotkeys::*;
use crate::settings_proc::*;
use crate::suggest_popup::*;
use crate::tray::*;
use crate::types::*;
use crate::updater::*;
use crate::user_dirs::*;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::RwLock;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, unbounded};
use global_hotkey::GlobalHotKeyManager;
use poltertype_core::audio::AudioPlayer;
use poltertype_core::engine::{
    DictionaryAddOrigin, EngineCommand, EngineDeps, SwitcherEngine, SwitcherEvent,
};
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::SettingsStore;
use poltertype_detect::Detector;
use poltertype_input::{
    KeyEvent, create_emitter, create_focus_tracker, create_key_gate, create_listener,
};
use poltertype_layout::{LayoutError, create_switcher};
use poltertype_popup::{PopupUiEvent, create_popup};
use poltertype_types::LayoutId;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tracing::{debug, error, info, warn};
use tray_icon::TrayIcon;
use tray_icon::TrayIconBuilder;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

/// How often a plug-in that reports state is re-asked while the user is
/// not touching the menu. Slow on purpose: every tick costs one
/// subprocess per reporting plug-in.
const PLUGIN_STATE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Move the mark on the tray icon to match what the plug-ins are waiting
/// on, redrawing only when the number changed: the icon is rasterised
/// from scratch on every redraw.
fn sync_attention(
    tray: &TrayIcon,
    item_pause: &tray_icon::menu::MenuItem,
    state: &mut TrayState,
    menu: &plugins::PluginMenu,
) {
    if state.attention == menu.attention() {
        return;
    }
    state.attention = menu.attention();
    tray::refresh_tray(tray, item_pause, state);
}

fn main() -> Result<()> {
    // Before `init_tracing` / single-instance on purpose: the settings
    // UI is a child process that would hit the lock and steal the
    // tray's log rotation, and `--help` / `--version` must stay cheap.
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "--settings" | "-s" | "settings" => return settings_ui::run(false),
            // The tray uses this when the keyboard hooks failed to
            // start, so the user lands on the one screen that helps.
            "--setup" => return settings_ui::run(true),
            "--plugins" => {
                return settings_ui::run_on(settings_ui::Pane::Plugins);
            }
            "--version" | "-V" => {
                println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("poltertype: unknown argument `{other}`");
                print_help();
                return Err(anyhow::anyhow!("unknown CLI argument"));
            }
        }
    }

    let _log_guard = init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "{APP_NAME} starting");

    // `single-instance` means something different by "id" on each OS —
    // on macOS a file path it flocks, which is why this is not just
    // `APP_ID`. See `poltertype_shell::instance_lock_id`.
    let config_dir = poltertype_core::settings::SettingsStore::project_dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|_| std::env::temp_dir());
    let Some(_instance) = poltertype_shell::acquire_instance_lock(APP_ID, &config_dir)
        .context("create single-instance lock")?
    else {
        warn!(
            "another instance is already running, exiting — if no PolterType window or tray \
             icon exists, look for a leftover PolterType or plug-in process"
        );
        return Ok(());
    };

    // ─── Settings ──────────────────────────────────────────────────
    let settings = match SettingsStore::load_or_default() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!(?e, "could not load settings; aborting startup");
            return Err(anyhow::anyhow!(e));
        }
    };
    info!(path = ?settings.path(), "settings loaded");

    // Runs on every startup, so a config edited by hand takes effect too.
    poltertype_autostart::sync(
        settings.snapshot().general.autostart,
        poltertype_autostart::App {
            id: APP_ID,
            name: APP_NAME,
            icon: poltertype_shell::DESKTOP_ID,
        },
    );

    // A menu entry and an icon, written only where nothing has installed
    // them already: without them a Wayland session has no icon for our
    // windows to wear at all.
    poltertype_shell::install_desktop_entry();

    // ─── Layout switcher (built first so we can query active OS
    //                     layouts before loading the DB) ────────────
    let layout_switcher: Arc<dyn poltertype_layout::LayoutSwitcher> = match switcher_with_retry() {
        Ok(s) => {
            info!(backend = s.backend_name(), "layout switcher ready");
            Arc::from(s)
        }
        Err(e) => {
            error!(?e, "no layout switcher backend; aborting");
            return Err(anyhow::anyhow!(e));
        }
    };

    // ─── Layouts ───────────────────────────────────────────────────
    // Data files are resolved at runtime — see `docs/DATA_LAYOUT.md`.
    // Only the layouts the OS reports as enabled are loaded: saves the
    // FST RAM and stops the detector picking an unreachable layout.
    let data_dir = poltertype_core::resolve_data_dir().context("resolve data directory")?;
    info!(?data_dir, "data directory resolved");

    let active_os_layouts = match layout_switcher.list_active() {
        Ok(list) => {
            info!(active = ?list, count = list.len(), "OS active layouts");
            Some(list)
        }
        Err(e) => {
            // Fail-open: load every bundled layout. The detector and
            // the `apply_correction` pre-flight still guard the target.
            warn!(
                ?e,
                "could not query active OS layouts; loading every bundled layout"
            );
            None
        }
    };

    // `list_active` names languages, but a language is not a keyboard —
    // Bulgarian alone has three under `bg-BG` and a bundled mapping can
    // describe only one. A backend that cannot answer returns nothing
    // and the bundled tables stand.
    let os_keymaps = match layout_switcher.describe_keymaps() {
        Ok(maps) => {
            info!(
                count = maps.len(),
                described = ?maps.iter().map(|m| (&m.id, &m.variant)).collect::<Vec<_>>(),
                "OS keyboard descriptions"
            );
            maps
        }
        Err(e) => {
            warn!(
                ?e,
                "could not describe OS keyboards; using bundled mappings as-is"
            );
            Vec::new()
        }
    };

    let user_wordlist_dir = poltertype_core::layouts::user_wordlist_dir();
    let user_layout_dir = poltertype_core::layouts::user_layout_dir();
    let layouts = Arc::new(
        LayoutDb::load(poltertype_core::layouts::LoadOptions {
            data_dir: Some(&data_dir),
            active_filter: active_os_layouts.as_deref(),
            user_layout_dir: user_layout_dir.as_deref(),
            user_wordlist_dir: user_wordlist_dir.as_deref(),
            os_keymaps: Some(&os_keymaps),
        })
        .context("load layout DB")?,
    );
    info!(
        loaded = layouts.len(),
        ids = ?layouts.ids().collect::<Vec<_>>(),
        wordlist_overlay = ?user_wordlist_dir,
        layout_overlay = ?user_layout_dir,
        "layout DB ready"
    );
    let key_emitter = match create_emitter() {
        Ok(e) => {
            info!(backend = e.backend_name(), "key emitter ready");
            Arc::from(e)
        }
        Err(e) => {
            warn!(?e, "no key emitter backend; corrections will be no-op");
            Arc::from(noop_emitter()) as Arc<dyn poltertype_input::KeyEmitter>
        }
    };
    // Created before the listener because on Linux/evdev the two share
    // the thread that owns the devices. Whether it can do anything is
    // decided once the listener starts — see `KeyGate::available`.
    let key_gate = create_key_gate();

    let audio = Arc::new(AudioPlayer::new());
    audio.refresh_from(&settings);

    let focus_tracker = create_focus_tracker();
    info!(
        backend = focus_tracker.backend_name(),
        "focus tracker ready"
    );

    // Dictionary first (highest signal, and it tie-breaks tokens that
    // look plausible either way), word-plausibility as the fallback.
    // The engine stops at the first non-NoOpinion verdict.
    let dictionary = build_dictionary_detector(&layouts);
    // Shares the inner `Arc<RwLock>` with the detector inside the engine,
    // so "Reload Settings" and the profile watcher can swap dictionaries
    // without a restart. The suggester takes another clone of the same
    // handle, so a swap reaches suggestions too.
    let dict_reload_handle = dictionary.handle();
    let suggester = build_suggester(&layouts, dictionary.handle());
    let mut detectors: Vec<Box<dyn Detector>> = vec![
        Box::new(dictionary),
        Box::new(build_plausibility_detector(&layouts)),
    ];
    // Appended, never substituted: an AI plug-in only adds a voice. The
    // gates it has to pass are in `detectors::build_ai_detectors`.
    detectors.extend(build_ai_detectors(&settings.snapshot().ai));

    // ── Wordlist profile cache + focus watcher ───────────────────────
    //
    // One dictionary set per configured profile, built up front: the
    // FSTs are already Arc-shared, so this only rebuilds the user
    // overlays. Shared so the settings close-handler can rebuild it from
    // disk; without that, per-profile edits would need a tray restart.
    let profile_dict_cache: ProfileDictCache = Arc::new(RwLock::new(build_full_profile_cache(
        &layouts,
        &data_dir,
        &settings.snapshot().wordlists,
        user_wordlist_dir.as_deref(),
    )));
    info!(
        profiles = profile_dict_cache.read().len(),
        "wordlist profile cache built (including global baseline)"
    );

    // Set by the close-handler after a rebuild so the watcher re-applies
    // on its next tick even though the resolved profile did not change.
    // Otherwise editing words while focused on a profiled app has no
    // effect until the user alt-tabs away and back.
    let profile_force_reapply: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // ─── Engine ────────────────────────────────────────────────────
    let (key_tx, key_rx) = bounded::<KeyEvent>(1024);
    let (engine_event_tx, engine_event_rx) = unbounded::<SwitcherEvent>();
    let (engine_cmd_tx, engine_cmd_rx) = unbounded::<EngineCommand>();

    // Cloned before the engine takes it: the layout poller publishes
    // LayoutChanged through the same channel.
    let engine_event_tx_for_poller = engine_event_tx.clone();

    let engine = SwitcherEngine::new(EngineDeps {
        settings: Arc::clone(&settings),
        layouts: Arc::clone(&layouts),
        detectors,
        layout_switcher: Arc::clone(&layout_switcher),
        key_emitter: Arc::clone(&key_emitter),
        key_gate: key_gate.clone(),
        focus_tracker: Arc::clone(&focus_tracker),
        audio: Arc::clone(&audio),
        out_tx: engine_event_tx,
        suggester: Some(suggester),
    });
    std::thread::Builder::new()
        .name("poltertype-engine".into())
        .spawn(move || engine.run(key_rx, engine_cmd_rx))
        .context("spawn engine thread")?;

    // ─── Input listener ────────────────────────────────────────────
    // A failure here turns off the app's whole reason to exist, so the
    // error text is kept and surfaced as an onboarding alert.
    let mut input_alert: Option<String> = None;
    let mut input_listener = match create_listener(&key_gate) {
        Ok(l) => Some(l),
        Err(e) => {
            warn!(
                ?e,
                "no input listener backend; engine will receive no events"
            );
            input_alert = Some(e.to_string());
            None
        }
    };
    if let Some(listener) = input_listener.as_mut() {
        match listener.start(key_tx) {
            Ok(()) => info!(
                backend = listener.backend_name(),
                holds_keys = key_gate.available(),
                "input listener started"
            ),
            Err(e) => {
                warn!(?e, "input listener failed to start");
                input_alert = Some(e.to_string());
            }
        }
    }

    // On Wayland/evdev the OS-level `global-hotkey` grab never sees
    // native input, but the evdev listener observes every key — so the
    // chords are detected off that stream instead. Never both paths for
    // one backend, so no double-fire.
    let use_keystream_hotkeys = input_listener
        .as_ref()
        .is_some_and(|l| l.backend_name() == "linux-wayland-evdev");

    // ─── Tao event loop + tray + global hotkeys ────────────────────
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    // Tray-only app: on macOS `LSUIElement` alone does not keep us out
    // of the Dock, because tao applies its own activation policy over
    // it. Must happen before `run`.
    poltertype_shell::keep_out_of_dock(&mut event_loop);

    let menu = Menu::new();
    // Only when the keyboard hooks failed to start.
    let item_setup = input_alert
        .as_ref()
        .map(|_| MenuItem::new("⚠ Keyboard hooks unavailable — Setup…", true, None));
    if let Some(item) = item_setup.as_ref() {
        menu.append_items(&[item, &PredefinedMenuItem::separator()])
            .context("populate tray alert entry")?;
    }
    let item_settings_ui = MenuItem::new("Settings…", true, None);
    let item_settings_file = MenuItem::new("Edit config.toml…", true, None);
    let item_logs = MenuItem::new("Open Logs Folder…", true, None);
    let item_wordlists = MenuItem::new("Open User Wordlists Folder…", true, None);
    let item_layouts = MenuItem::new("Open User Layouts Folder…", true, None);
    let item_reload = MenuItem::new("Reload Settings", true, None);
    let item_pause = MenuItem::new("Pause auto-switch", true, None);

    // One dual-purpose entry — "Check for updates…" or "⟳ Restart to
    // update"; hidden entirely when `[updates].enabled` is off.
    let updates_enabled = settings.snapshot().updates.enabled;
    let mut update_pending = if updates_enabled {
        pending_for_this_build()
    } else {
        None
    };
    let item_update =
        updates_enabled.then(|| MenuItem::new(menu_label(update_pending.as_ref()), true, None));

    let item_about = MenuItem::new(
        format!("About {APP_NAME} v{}", env!("CARGO_PKG_VERSION")),
        false,
        None,
    );
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[
        &item_settings_ui,
        &item_settings_file,
        &item_logs,
        &item_wordlists,
        &item_layouts,
        &item_reload,
        &PredefinedMenuItem::separator(),
        &item_pause,
        &PredefinedMenuItem::separator(),
    ])
    .context("populate tray menu")?;
    if let Some(item) = item_update.as_ref() {
        menu.append_items(&[item, &PredefinedMenuItem::separator()])
            .context("populate tray update entry")?;
    }
    menu.append_items(&[&item_about, &item_quit])
        .context("populate tray menu tail")?;

    // Plug-ins last, so the app's own entries keep their position and
    // a plug-in can never push Quit off the bottom of the menu.
    let discovered = poltertype_core::plugins::extensions(&data_dir);
    let mut plugin_menu = plugins::PluginMenu::build(discovered, &menu)?;
    let mut supervisor = plugins::Supervisor::new();
    supervisor.start_all(plugin_menu.extensions());
    for ext in plugin_menu.extensions() {
        info!(
            id = %ext.id,
            version = %ext.version,
            development = ext.development,
            service = supervisor.is_running(&ext.id),
            "plug-in loaded"
        );
    }

    let setup_id = item_setup.as_ref().map(|i| i.id().clone());
    let update_id = item_update.as_ref().map(|i| i.id().clone());
    let settings_ui_id = item_settings_ui.id().clone();
    let settings_file_id = item_settings_file.id().clone();
    let logs_id = item_logs.id().clone();
    let wordlists_id = item_wordlists.id().clone();
    let layouts_id = item_layouts.id().clone();
    let reload_id = item_reload.id().clone();
    let pause_id = item_pause.id().clone();
    let quit_id = item_quit.id().clone();

    // Initial icon: query the OS for the current layout so we don't
    // flash a "??" before the first LayoutChanged event arrives.
    let initial_layout: Option<LayoutId> = layout_switcher.current().ok();
    let initial_icon = match initial_layout.as_ref() {
        Some(l) => icon_render::for_layout(l, false, false)?,
        None => icon_render::unknown(false)?,
    };

    // Ask before building, not after: on Linux the tray library is
    // `dlopen`ed, and its absence aborts the process from inside a
    // dependency with a message nobody can act on. See `poltertype-tray`.
    if let Some(reason) = poltertype_tray::unavailable_reason() {
        error!("{reason}");
        anyhow::bail!("system tray unavailable");
    }

    // Before the tray exists: the GTK backend greets its construction
    // with a deprecation warning meant for whoever links it, not for
    // the user reading the journal. See `poltertype-tray`.
    poltertype_tray::quiet_gtk_tray_logs();

    let tray: TrayIcon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip_for(
            initial_layout.as_ref(),
            false,
            input_alert.is_some(),
            0,
        ))
        .with_icon(initial_icon)
        .build()
        .context("build tray icon")?;

    // Deliberately on the error path, not gated by
    // `show_notifications`: without hooks the app silently does nothing.
    if let Some(reason) = input_alert.as_deref() {
        spawn_error_notification(format!(
            "Keyboard hooks are unavailable — automatic layout switching is off.\n\
             {reason}\n\
             Tray menu → \"Setup…\" shows what is missing and how to fix it."
        ));
    }

    // `MenuItem` is internally Arc-shared, so this clone is a refcount.
    let item_pause_for_loop = item_pause.clone();

    let hotkey_manager = GlobalHotKeyManager::new().context("create global-hotkey manager")?;
    // Built from the live backends rather than probed: the tray knows
    // exactly what it started. The Settings window has neither backend
    // and probes instead — both then run the same resolver, which is
    // the only thing keeping the two from disagreeing (issue #31).
    let hk_env = poltertype_input::HotkeyEnvironment {
        observed_not_consumed: use_keystream_hotkeys,
        system_owns_ctrl_shift_space: layout_switcher.backend_name() == "macos-tis",
    };
    let snap = settings.snapshot();
    let pause = effective_pause_toggle(&snap.hotkeys.pause_toggle, hk_env);
    let switch = effective_switch_last(&snap.hotkeys.manual_switch_last, hk_env);
    if pause.substitution.is_some() {
        info!(
            rebound_to = pause.chord,
            "macOS: default pause ({DEFAULT_PAUSE_TOGGLE}) is the system input-source shortcut; using a free chord"
        );
    }
    if switch.substitution.is_some() {
        info!(
            rebound_to = switch.chord,
            "Wayland: default switch-last ({DEFAULT_SWITCH_LAST}) is destructive in-app; using a safe key"
        );
    }
    let hk_pause = parse_hotkey_or_default(pause.chord, DEFAULT_PAUSE_TOGGLE);
    let hk_switch = parse_hotkey_or_default(switch.chord, DEFAULT_SWITCH_LAST);
    if use_keystream_hotkeys {
        let chords = poltertype_core::engine::KeystreamHotkeys {
            pause: chord_from_hotkey(&hk_pause),
            switch_last: chord_from_hotkey(&hk_switch),
        };
        if chords.pause.is_none() {
            warn!(hotkey = ?hk_pause, "pause hotkey key not mappable to a scancode; disabled");
        }
        if chords.switch_last.is_none() {
            warn!(hotkey = ?hk_switch, "switch-last hotkey key not mappable to a scancode; disabled");
        }
        let _ = engine_cmd_tx.send(EngineCommand::SetKeystreamHotkeys(chords));
        info!("hotkeys handled off the key stream (Wayland/evdev backend)");
    } else {
        if let Err(e) = hotkey_manager.register(hk_pause) {
            warn!(?e, hotkey = ?hk_pause, "could not register pause hotkey");
        }
        if let Err(e) = hotkey_manager.register(hk_switch) {
            warn!(?e, hotkey = ?hk_switch, "could not register switch-last hotkey");
        }
    }
    let pause_hotkey_id = hk_pause.id();
    let switch_hotkey_id = hk_switch.id();

    // Smart commands are text triggers consulted on every word
    // boundary, never global hotkeys — see `poltertype_core::commands`.

    spawn_event_bridges(event_loop.create_proxy(), engine_event_rx.clone())?;

    // Suggestion tooltip. The backend spawns its own thread (or is a
    // noop on platforms without an overlay path); clicks and timeouts
    // come back through the popup bridge as `UserEvent::Popup`.
    let (popup_event_tx, popup_event_rx) = unbounded::<PopupUiEvent>();
    let popup = create_popup(popup_event_tx);
    spawn_popup_bridge(event_loop.create_proxy(), popup_event_rx)?;
    let focus_for_popup = Arc::clone(&focus_tracker);

    // The worker owns every network call this app makes; the event loop
    // only sees results. `check_now_tx` cuts its sleep short, bounded at
    // 1 because a double click wants one check, not a queue.
    let (check_now_tx, check_now_rx) = bounded::<()>(1);
    if updates_enabled {
        spawn_update_worker(
            event_loop.create_proxy(),
            Arc::clone(&settings),
            check_now_rx,
        )?;
    } else {
        info!("automatic updates are disabled in config.toml; no update checks will be made");
    }

    spawn_layout_poller(Arc::clone(&layout_switcher), engine_event_tx_for_poller)?;

    if !profile_dict_cache.read().is_empty() {
        spawn_profile_watcher(
            Arc::clone(&focus_tracker),
            Arc::clone(&settings),
            Arc::clone(&profile_dict_cache),
            Arc::clone(&profile_force_reapply),
            dict_reload_handle.handle(),
        )?;
    }

    let settings_path: PathBuf = settings.path().to_owned();
    let log_dir: Option<PathBuf> = SettingsStore::log_dir().ok();
    let cmd_tx_for_loop = engine_cmd_tx.clone();
    let settings_for_loop = Arc::clone(&settings);

    let mut tray_state = TrayState {
        layout: initial_layout,
        paused: false,
        input_alert: input_alert.is_some(),
        attention: 0,
    };

    info!("entering event loop");
    // A slow heartbeat, so a mode changed from the command line — or an
    // authority that expired on its own — reaches the menu without a
    // click. A thread rather than `ControlFlow::WaitUntil`: the GTK
    // backend never delivers the timed wake-up, and the timer version
    // silently never fired.
    //
    // Armed only when there is a plug-in to watch. A service counts even
    // if it reports no state: this is also the only thing that notices
    // one dying.
    if plugin_menu.reports_state() || supervisor.has_services() {
        let proxy = event_loop.create_proxy();
        std::thread::Builder::new()
            .name("plugin-state".into())
            .spawn(move || {
                while proxy.send_event(UserEvent::PluginState).is_ok() {
                    std::thread::sleep(PLUGIN_STATE_INTERVAL);
                }
            })
            .context("cannot start the plug-in state heartbeat")?;
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::PluginState) => {
                // Before the menu refresh, not after: a plug-in's state
                // command answers the same dead or alive, so the tray
                // would keep showing a mode nothing is enforcing.
                announce_departed(supervisor.reap());
                plugin_menu.refresh();
                sync_attention(&tray, &item_pause, &mut tray_state, &plugin_menu);
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                announce_departed(supervisor.reap());
                if plugin_menu.handle(&id) {
                    // Belonged to a plug-in, which has re-read its state:
                    // the mark has to move with it, or clearing a queue
                    // looks like a click that did nothing for another
                    // fifteen seconds.
                    sync_attention(&tray, &item_pause, &mut tray_state, &plugin_menu);
                } else if id == quit_id {
                    info!("Quit clicked — shutting down");
                    if let Some(mut listener) = input_listener.take() {
                        listener.stop();
                    }
                    // Before anything on disk is replaced: a plug-in
                    // service still running through an update would be
                    // a process whose binary moved under it.
                    supervisor.stop_all();
                    // The one safe moment: the user is done typing, the
                    // hook is down, and nothing we replace is in use. No
                    // relaunch — they asked for the app to go away.
                    if let Some(pending) = update_pending.as_ref() {
                        apply_now(pending, false);
                    }
                    *control_flow = ControlFlow::Exit;
                } else if Some(&id) == update_id.as_ref() {
                    match update_pending.as_ref() {
                        Some(pending) => {
                            info!(version = %pending.version, "Restart to update clicked");
                            if let Some(mut listener) = input_listener.take() {
                                listener.stop();
                            }
                            apply_now(pending, true);
                            *control_flow = ControlFlow::Exit;
                        }
                        None => {
                            info!("manual update check");
                            let _ = check_now_tx.try_send(());
                        }
                    }
                } else if id == settings_ui_id {
                    spawn_settings_ui(SettingsCloseDeps {
                        settings: Arc::clone(&settings_for_loop),
                        layouts: Arc::clone(&layouts),
                        data_dir: data_dir.clone(),
                        user_wordlist_dir: user_wordlist_dir.clone(),
                        dict_reload_handle: dict_reload_handle.handle(),
                        profile_dict_cache: Arc::clone(&profile_dict_cache),
                        profile_force_reapply: Arc::clone(&profile_force_reapply),
                        reload_tx: cmd_tx_for_loop.clone(),
                    });
                } else if id == settings_file_id {
                    open_path(&settings_path, "settings file");
                } else if id == logs_id {
                    if let Some(dir) = log_dir.as_ref() {
                        let _ = std::fs::create_dir_all(dir);
                        open_path(dir, "log directory");
                    } else {
                        warn!("log directory unknown");
                    }
                } else if id == wordlists_id {
                    match ensure_user_wordlist_dir() {
                        Ok(dir) => open_path(&dir, "user wordlists folder"),
                        Err(e) => warn!(?e, "could not prepare user wordlists folder"),
                    }
                } else if id == layouts_id {
                    // New layouts here are picked up only on app restart.
                    match ensure_user_layout_dir() {
                        Ok(dir) => open_path(&dir, "user layouts folder"),
                        Err(e) => warn!(?e, "could not prepare user layouts folder"),
                    }
                } else if id == reload_id {
                    // Also re-reads the user overlays, which is what
                    // lets added vocabulary apply without a restart.
                    let reloaded_dicts = reload_user_dictionaries(&dict_reload_handle);
                    match settings_for_loop.reload() {
                        Ok(changed) => {
                            info!(
                                config_changed = changed,
                                dicts_reloaded = reloaded_dicts,
                                "Reload Settings"
                            );
                            if changed {
                                let _ = cmd_tx_for_loop.send(EngineCommand::SettingsReloaded);
                            }
                        }
                        Err(e) => warn!(?e, "could not reload config.toml"),
                    }
                } else if id == pause_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::TogglePause);
                } else if Some(&id) == setup_id.as_ref() {
                    // The Setup pane, not a browser tab: it says what is
                    // missing *on this machine* and re-checks after the
                    // user has fixed it.
                    spawn_setup_ui(SettingsCloseDeps {
                        settings: Arc::clone(&settings_for_loop),
                        layouts: Arc::clone(&layouts),
                        data_dir: data_dir.clone(),
                        user_wordlist_dir: user_wordlist_dir.clone(),
                        dict_reload_handle: dict_reload_handle.handle(),
                        profile_dict_cache: Arc::clone(&profile_dict_cache),
                        profile_force_reapply: Arc::clone(&profile_force_reapply),
                        reload_tx: cmd_tx_for_loop.clone(),
                    });
                }
            }
            Event::UserEvent(UserEvent::Hotkey(id)) => {
                if id == pause_hotkey_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::TogglePause);
                } else if id == switch_hotkey_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::SwitchLastForcefully);
                }
            }
            Event::UserEvent(UserEvent::Engine(ev)) => match ev {
                SwitcherEvent::SuggestionsReady {
                    generation,
                    original,
                    entries,
                    timeout,
                    accept_modifiers,
                } => {
                    show_suggestion_popup(
                        popup.as_ref(),
                        &focus_for_popup,
                        generation,
                        original,
                        entries,
                        timeout,
                        accept_modifiers,
                    );
                }
                SwitcherEvent::SuggestionsDismissed { .. } => popup.hide(),
                SwitcherEvent::SuggestionApplied { .. } => {
                    // The engine already played the sound; the tooltip
                    // hid on click. Nothing tray-side to update, and
                    // the replacement text stays out of the logs.
                    info!("suggestion applied");
                }
                SwitcherEvent::AddToDictionary {
                    layout,
                    word,
                    origin,
                } => {
                    match add_word_to_user_overlay(&layout, &word, &dict_reload_handle) {
                        // Only the implicit route announces itself —
                        // see `spawn_dictionary_add_notification`.
                        Ok(()) => {
                            if origin == DictionaryAddOrigin::UndoneCorrection
                                && settings_for_loop.snapshot().general.show_notifications
                            {
                                spawn_dictionary_add_notification(&layouts, &layout, &word);
                            }
                        }
                        Err(e) => {
                            warn!(?e, "could not add the word to the user wordlist overlay");
                        }
                    }
                }
                other => handle_engine_event(
                    other,
                    &tray,
                    &item_pause_for_loop,
                    &mut tray_state,
                    &settings_for_loop,
                    &layouts,
                ),
            },
            Event::UserEvent(UserEvent::Popup(pe)) => match pe {
                PopupUiEvent::Accepted { generation, index } => {
                    let _ = cmd_tx_for_loop.send(EngineCommand::AcceptSuggestion {
                        typed_digit: false,
                        generation,
                        index,
                        from_pointer: true,
                    });
                }
                PopupUiEvent::TimedOut { generation } => {
                    let _ = cmd_tx_for_loop.send(EngineCommand::DismissSuggestions { generation });
                }
            },
            Event::UserEvent(UserEvent::Update(outcome)) => {
                match outcome {
                    UpdateOutcome::Staged(pending) => {
                        // Once, on the transition — the worker stages a
                        // given version only once, so nobody is nagged.
                        let already_known = update_pending
                            .as_ref()
                            .is_some_and(|p| p.version == pending.version);
                        update_pending = Some(*pending);
                        if !already_known {
                            if let Some(p) = update_pending.as_ref() {
                                spawn_update_notification(&p.version);
                            }
                        }
                    }
                    UpdateOutcome::UpToDate | UpdateOutcome::Cleared => update_pending = None,
                    // Already logged by the worker. A failed check is not
                    // news, and it must not cost a user the button to
                    // install an update that is already staged.
                    UpdateOutcome::Failed => {}
                }
                if let Some(item) = item_update.as_ref() {
                    refresh_menu_item(item, update_pending.as_ref());
                }
            }
            _ => {}
        }
    });
}

/// Tell the user a plug-in service is gone, once, as it happens.
///
/// A stopped plug-in is invisible by construction: its tray entries are
/// one-shot commands that keep working, so the menu looks identical
/// whether the service behind it is running or dead. On the
/// error-notification path, so `show_notifications` does not gate it.
/// [`create_switcher`], retried for a few seconds before giving up.
///
/// At login we can be started before the session has anything to
/// probe. That is not hypothetical: an `xdg-desktop-autostart` unit
/// beat the Hyprland session's own environment import, PolterType
/// probed seven backends, found none and exited 1 — so "run at login"
/// simply did not work, with the reason in a journal nobody reads.
///
/// Patience costs a genuinely unsupported machine a slower error
/// message, and buys every autostarted one a working app.
fn switcher_with_retry() -> Result<Box<dyn poltertype_layout::LayoutSwitcher>, LayoutError> {
    let deadline = Instant::now() + SWITCHER_PROBE_WINDOW;
    loop {
        match create_switcher() {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => {
                debug!("no layout switcher backend yet; the session may still be coming up");
                std::thread::sleep(SWITCHER_PROBE_INTERVAL);
            }
        }
    }
}

fn announce_departed(gone: Vec<plugins::Departed>) {
    for d in gone {
        spawn_error_notification(format!(
            "The {id} plug-in stopped and will not restart on its own.\n\
             {why}\n\
             Its own log is next to PolterType's, as plugin-{id}.log.",
            id = d.id,
            why = d.why,
        ));
    }
}

fn print_help() {
    println!(
        "{APP_NAME} {ver}\n\
        \n\
        USAGE:\n  \
            poltertype              start the tray app\n  \
            poltertype --settings   open the settings window\n  \
            poltertype --setup      open the settings window on the Setup pane\n  \
            poltertype --version    print version and exit\n  \
            poltertype --help       show this help",
        ver = env!("CARGO_PKG_VERSION"),
    );
}

/// Init `tracing` with a stderr layer and a daily-rotating file appender
/// under `<data_dir>/poltertype/logs/`. Returns the file writer's guard —
/// dropping it closes the file.
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, fmt};

    // cosmic-text / fontdb log the *text being shaped* at debug level
    // ("Failed to find script fallback …: '<word>'") — and the
    // suggestion tooltip shapes the user's words. Those targets are
    // capped at warn no matter what RUST_LOG says: typed text stays
    // out of the logs at any level.
    let mut filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    for target in ["cosmic_text=warn", "fontdb=warn"] {
        if let Ok(directive) = target.parse() {
            filter = filter.add_directive(directive);
        }
    }

    let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_target(false);

    let (file_layer, guard) = match SettingsStore::log_dir() {
        Ok(dir) => {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("poltertype: could not create log dir {dir:?}: {e}");
                (None, None)
            } else {
                let appender = tracing_appender::rolling::daily(&dir, "poltertype.log");
                let (writer, guard) = tracing_appender::non_blocking(appender);
                let layer = fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false)
                    .with_target(false);
                (Some(layer), Some(guard))
            }
        }
        Err(e) => {
            eprintln!("poltertype: cannot resolve log dir: {e}");
            (None, None)
        }
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    guard
}
