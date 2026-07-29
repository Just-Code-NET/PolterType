//! poltertype application entry point.
//!
//! Wires the tray + global keyboard listener + layout switcher +
//! `SwitcherEngine` together, registers the two built-in global
//! hotkeys (pause / switch-last), and spawns the focus-driven
//! wordlist-profile watcher when the user has profiles configured.
//!
//! The Settings GUI is a **separate process** spawned via
//! `poltertype --settings` — see `settings_ui.rs` for the
//! rationale (macOS main-thread contention, crash isolation).
//! User-defined "smart commands" (`[[commands]]` in `config.toml`)
//! are NOT wired here as global hotkeys; they're text triggers
//! consulted by the engine on every word boundary. See
//! `poltertype_core::commands` for the design.

#![forbid(unsafe_code)]

mod icon_render;
mod settings_ui;

mod autostart;
mod bridges;
mod consts;
mod detectors;
mod enums;
mod hotkeys;
mod settings_proc;
mod suggest_popup;
mod tray;
mod types;
mod updater;
mod user_dirs;

use crate::bridges::*;
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
use poltertype_core::engine::{EngineCommand, SwitcherEngine, SwitcherEvent};
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::SettingsStore;
use poltertype_detect::Detector;
use poltertype_input::{
    KeyEvent, create_emitter, create_focus_tracker, create_key_gate, create_listener,
};
use poltertype_layout::create_switcher;
use poltertype_popup::{PopupUiEvent, create_popup};
use poltertype_types::LayoutId;
use single_instance::SingleInstance;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tracing::{error, info, warn};
use tray_icon::TrayIcon;
use tray_icon::TrayIconBuilder;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

fn main() -> Result<()> {
    // CLI dispatch: `poltertype --settings` opens the Settings GUI
    // and exits when the window closes. Anything else falls through
    // to the tray. We do this BEFORE `init_tracing` / single-instance
    // because:
    //
    // * The settings UI is a short-lived child process spawned by the
    //   tray. Hitting the single-instance lock would kill it on
    //   startup; logging would steal the tray's log file rotation.
    // * `--help` / `--version` need to be cheap and side-effect-free.
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "--settings" | "-s" | "settings" => return settings_ui::run(),
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

    // On macOS the `single-instance` crate treats the id as a file
    // path and flocks it. A bare id lands in the process cwd — which
    // is `/` (read-only system volume) when the app is launched via
    // Finder / `open`, so startup died with "Read-only file system".
    // Give it an absolute path under the per-user config dir instead.
    // On Linux (abstract socket) and Windows (named mutex) the id is
    // not a path, so keep it untouched there.
    #[cfg(target_os = "macos")]
    let lock_id: String = {
        let dir = poltertype_core::settings::SettingsStore::project_dirs()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|_| std::env::temp_dir());
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!(?e, ?dir, "could not create config dir for instance lock");
        }
        dir.join(format!("{APP_ID}.lock"))
            .to_string_lossy()
            .into_owned()
    };
    #[cfg(not(target_os = "macos"))]
    let lock_id: &str = APP_ID;
    let instance = SingleInstance::new(&lock_id).context("create single-instance lock")?;
    if !instance.is_single() {
        warn!("another instance is already running, exiting");
        return Ok(());
    }

    // ─── Settings ──────────────────────────────────────────────────
    let settings = match SettingsStore::load_or_default() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!(?e, "could not load settings; aborting startup");
            return Err(anyhow::anyhow!(e));
        }
    };
    info!(path = ?settings.path(), "settings loaded");

    // Make the OS autostart entry match the setting (macOS
    // LaunchAgent; no-op elsewhere for now). Runs on every startup so
    // a config edited by hand takes effect too.
    autostart::sync(settings.snapshot().general.autostart);

    // ─── Layout switcher (built first so we can query active OS
    //                     layouts before loading the DB) ────────────
    let layout_switcher: Arc<dyn poltertype_layout::LayoutSwitcher> = match create_switcher() {
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
    // We now ship layout mappings + FST wordlists as plain files in
    // a `data/` directory next to the executable (Windows MSI),
    // inside `Contents/Resources/data/` (macOS .app), or
    // `usr/share/poltertype/data/` (Linux AppImage). The runtime
    // resolver in `poltertype_core::data_dir` figures out which path is
    // live; in dev mode it falls back to `target/dist/data/` where
    // `poltertype-core/build.rs` writes prepared assets.
    //
    // We then ask the OS which layouts the user has actually
    // enabled (`list_active`) and only load **those** wordlists into
    // memory. A user with `en-US / uk-UA / ru-RU` saves the FST RAM
    // for the four other bundled languages they'd never query — and
    // the detector can no longer pick an unreachable layout (the
    // root cause of the original `http ` bug).
    let data_dir = poltertype_core::resolve_data_dir().context("resolve data directory")?;
    info!(?data_dir, "data directory resolved");

    let active_os_layouts = match layout_switcher.list_active() {
        Ok(list) => {
            info!(active = ?list, count = list.len(), "OS active layouts");
            Some(list)
        }
        Err(e) => {
            // Fail-open: we can't decide what's reachable, so load
            // every bundled layout (the previous baked-in behaviour).
            // The detector + apply_correction pre-flight guard will
            // still catch any unreachable target at runtime.
            warn!(
                ?e,
                "could not query active OS layouts; loading every bundled layout"
            );
            None
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
    // Holds the user's keystrokes back while a correction is typed, so
    // nothing of theirs lands in the middle of it. Created before the
    // listener because on Linux/evdev the two share the thread that
    // owns the devices; whether it can do anything is decided once the
    // listener starts (see `KeyGate::available`).
    let key_gate = create_key_gate();

    let audio = Arc::new(AudioPlayer::new());
    audio.refresh_from(&settings);

    let focus_tracker = create_focus_tracker();
    info!(
        backend = focus_tracker.backend_name(),
        "focus tracker ready"
    );

    // Detector pipeline: dictionary first (highest signal — catches
    // single-letter prepositions and tie-breaks "both look plausible"
    // tokens), word-plausibility second as a fallback for tokens that
    // aren't in either dictionary. Both are pure functions; engine
    // runs them in order and stops at the first non-NoOpinion verdict.
    let dictionary = build_dictionary_detector(&layouts);
    // Cloned handle — shares the inner Arc<RwLock> with the
    // detector that lives inside the engine. Used by the
    // "Reload Settings" path to swap in fresh dictionaries
    // (re-reading user-overlay files) without restarting, AND by
    // the focus-driven wordlist profile watcher below to swap
    // per-app overlays as the user moves between editors / chat /
    // browser / IDE.
    let dict_reload_handle = dictionary.handle();
    // The suggester shares the same hot-swappable dict set through
    // another handle clone — per-app profile swaps and settings
    // reloads reach suggestions without any extra plumbing.
    let suggester = build_suggester(&layouts, dictionary.handle());
    let detectors: Vec<Box<dyn Detector>> = vec![
        Box::new(dictionary),
        Box::new(build_plausibility_detector(&layouts)),
    ];

    // ── Wordlist profile cache + focus watcher ───────────────────────
    //
    // Build one dictionary set per configured `[[wordlists.profiles]]`
    // entry up front. The FSTs are already Arc-shared inside
    // LayoutDictionary, so this is "rebuild the user-overlay HashSets
    // once per profile" — milliseconds, even for 5+ profiles.
    //
    // The focus watcher thread (spawned right after the engine is
    // running) polls `focus_tracker.focused_exe()` every ~250 ms,
    // resolves the active profile via `wordlist_profiles::resolve`,
    // and atomically swaps the dictionary set when it changes. The
    // swap is a single `RwLock::write()` — same primitive the manual
    // "Reload Settings" path uses.
    // Profile cache is shared (Arc<RwLock>) so the close-handler in
    // `spawn_settings_ui` can rebuild it from disk when the user
    // saves wordlist edits via the GUI; without that, per-profile
    // wordlist edits would only apply after a tray restart.
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

    // Force-reapply flag: set by the close-handler after rebuilding
    // the cache so the watcher re-applies on its next tick (~250 ms)
    // even though the resolved profile didn't change. Without this
    // the watcher only swaps on profile transitions, which means a
    // user editing words while focused on a profiled app would see
    // no effect until they alt-tabbed away and back.
    let profile_force_reapply: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // ─── Engine ────────────────────────────────────────────────────
    let (key_tx, key_rx) = bounded::<KeyEvent>(1024);
    let (engine_event_tx, engine_event_rx) = unbounded::<SwitcherEvent>();
    let (engine_cmd_tx, engine_cmd_rx) = unbounded::<EngineCommand>();

    // Clone the event sender before handing it to the engine — the
    // layout poller below also publishes LayoutChanged events through
    // the same channel.
    let engine_event_tx_for_poller = engine_event_tx.clone();

    let engine = SwitcherEngine::new(
        Arc::clone(&settings),
        Arc::clone(&layouts),
        detectors,
        Arc::clone(&layout_switcher),
        Arc::clone(&key_emitter),
        key_gate.clone(),
        Arc::clone(&focus_tracker),
        Arc::clone(&audio),
        engine_event_tx,
        Some(suggester),
    );
    std::thread::Builder::new()
        .name("poltertype-engine".into())
        .spawn(move || engine.run(key_rx, engine_cmd_rx))
        .context("spawn engine thread")?;

    // ─── Input listener ────────────────────────────────────────────
    // A failure here means the app's whole reason to exist is off —
    // so besides the log line we keep the error text and surface it
    // as an onboarding alert: tooltip suffix, a "Setup Guide" tray
    // menu entry, and a one-shot notification. A log file the user
    // has never heard of is not a user interface.
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

    // On the Wayland/evdev backend the OS-level `global-hotkey` grab
    // never sees native input (it can only bind through Xwayland, which
    // Hyprland & friends don't route real keystrokes into). The evdev
    // listener, however, observes every key — so we detect the hotkey
    // chords straight off that stream instead. We never run both paths
    // for one backend, so there's no double-fire.
    let use_keystream_hotkeys = input_listener
        .as_ref()
        .is_some_and(|l| l.backend_name() == "linux-wayland-evdev");

    // ─── Tao event loop + tray + global hotkeys ────────────────────
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let menu = Menu::new();
    // Onboarding alert entry — present only when keyboard hooks
    // failed to start (Wayland without the `input` group, X11 connect
    // refused, macOS without Accessibility). Clicking opens the
    // setup guide (docs/PERMISSIONS.md) in the browser.
    let item_setup = input_alert
        .as_ref()
        .map(|_| MenuItem::new("⚠ Keyboard hooks unavailable — Setup Guide…", true, None));
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

    // Updates. A single dual-purpose entry: while nothing is staged it
    // reads "Check for updates…" and forces a check; once the worker has
    // downloaded and verified a release it becomes "⟳ Restart to update
    // — v0.4.0" and installing is one click. Present only when
    // `[updates].enabled` — a user who turned updates off should not
    // have to keep looking at the machinery they switched off.
    //
    // A staged update may already exist at this point: the user could
    // have downloaded it in a previous session and then closed the app
    // without quitting through the menu (logout, reboot, `pkill`).
    // `pending_for_this_build` is what recovers it — and what throws it
    // away if it turns out to be the update this very process *is*.
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
        Some(l) => icon_render::for_layout(l, false)?,
        None => icon_render::unknown()?,
    };

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
        ))
        .with_icon(initial_icon)
        .build()
        .context("build tray icon")?;

    // One-shot startup notification for the same failure. Uses the
    // error-notification path (NOT gated by `show_notifications`):
    // without hooks the app silently does nothing, which is exactly
    // the "user waits for something that never happens" case.
    if let Some(reason) = input_alert.as_deref() {
        spawn_error_notification(format!(
            "Keyboard hooks are unavailable — automatic layout switching is off.\n\
             {reason}\n\
             Tray menu → \"Setup Guide\" explains the fix."
        ));
    }

    // Cloned reference into the event loop so we can flip the menu
    // item's text between "⏸ Pause auto-switch" and "▶ Resume
    // auto-switch" when the engine reports a state change. MenuItem
    // is internally Arc-shared, so this clone bumps a refcount.
    let item_pause_for_loop = item_pause.clone();

    // Global hotkeys — strings come from `[hotkeys]` in config.toml.
    // We parse them with `global-hotkey`'s `FromStr` (see
    // `parse_hotkey_or_default`); on a malformed entry we fall back to
    // the documented default so the user never ends up with a tray app
    // that silently lost its hotkeys after a typo.
    let hotkey_manager = GlobalHotKeyManager::new().context("create global-hotkey manager")?;
    let hk_pause = parse_hotkey_or_default(
        &settings.snapshot().hotkeys.pause_toggle,
        "Ctrl+Shift+Space",
    );
    // Switch-last default is backend-dependent. The cross-platform
    // default `Ctrl+Shift+Backspace` is fine where the OS *consumes* the
    // hotkey (Windows/X11), but on the Wayland keystream path we can
    // only *observe* keys — the Backspace also reaches the focused app,
    // where `Ctrl+Backspace` means "delete the previous word" and
    // corrupts the very text we're about to correct. So when the user
    // hasn't moved off that default, rebind to a key with no
    // destructive in-app effect. An explicit custom binding is honoured
    // as-is.
    let configured_switch = settings.snapshot().hotkeys.manual_switch_last;
    let switch_src = if use_keystream_hotkeys && configured_switch == DEFAULT_SWITCH_LAST {
        info!(
            rebound_to = WAYLAND_SAFE_SWITCH_LAST,
            "Wayland: default switch-last ({DEFAULT_SWITCH_LAST}) is destructive in-app; using a safe key"
        );
        WAYLAND_SAFE_SWITCH_LAST
    } else {
        &configured_switch
    };
    let hk_switch = parse_hotkey_or_default(switch_src, DEFAULT_SWITCH_LAST);
    if use_keystream_hotkeys {
        // Wayland: feed resolved chords to the engine; it matches them
        // off the evdev stream. Unmappable keys (no SC Set-1 equivalent
        // in our table) are dropped with a warning rather than failing.
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

    // User-defined "smart commands" (text triggers like `anrl ` →
    // `Anatomical Reference List`) are NOT registered as global
    // hotkeys — they're consulted by the engine on every word
    // boundary. See `poltertype_core::commands` for the architecture and
    // `SwitcherEngine::decide` for the dispatch path.

    spawn_event_bridges(event_loop.create_proxy(), engine_event_rx.clone())?;

    // Suggestion tooltip. The backend spawns its own thread (or is a
    // noop on platforms without an overlay path); clicks and timeouts
    // come back through the popup bridge as `UserEvent::Popup`.
    let (popup_event_tx, popup_event_rx) = unbounded::<PopupUiEvent>();
    let popup = create_popup(popup_event_tx);
    spawn_popup_bridge(event_loop.create_proxy(), popup_event_rx)?;
    let focus_for_popup = Arc::clone(&focus_tracker);

    // Background updates. The worker owns every network call this app
    // makes; the event loop only ever sees the result. `check_now_tx`
    // is how the tray's "Check for updates…" click cuts the worker's
    // sleep short — bounded at 1 because a user clicking twice wants one
    // check, not a queue of them.
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

    // Layout poller: the engine emits LayoutChanged for switches it
    // performs itself, but we miss user-driven manual switches (Win+
    // Space / Alt+Shift / language bar / ibus / kde-keyboard). Polling
    // the OS-level current-layout query every ~250 ms catches those
    // cheaply and keeps the tray icon in sync.
    spawn_layout_poller(Arc::clone(&layout_switcher), engine_event_tx_for_poller)?;

    // Focus-driven wordlist profile watcher: same cadence as the
    // layout poller. Cheap when no profiles are configured (the
    // profile-cache HashMap is empty so the swap path is a no-op).
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

    // Tray-side mirror of engine state. Updated from PausedChanged
    // and LayoutChanged events; consulted whenever we need to redraw
    // (icon + tooltip both depend on both fields).
    let mut tray_state = TrayState {
        layout: initial_layout,
        paused: false,
        input_alert: input_alert.is_some(),
    };

    info!("entering event loop");
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Menu(id)) => {
                if id == quit_id {
                    info!("Quit clicked — shutting down");
                    if let Some(mut listener) = input_listener.take() {
                        listener.stop();
                    }
                    // Quit is the moment we have been waiting for: the
                    // user is done typing, the hook is down, and nothing
                    // we replace can be in use. This is what "installs
                    // on restart" actually means — the install happens
                    // now, and the version they launch next is the new
                    // one. No relaunch: they asked for the app to go
                    // away, and an updater that reopens it would be
                    // overriding a direct instruction.
                    if let Some(pending) = update_pending.as_ref() {
                        apply_now(pending, false);
                    }
                    *control_flow = ControlFlow::Exit;
                } else if Some(&id) == update_id.as_ref() {
                    match update_pending.as_ref() {
                        // Staged and verified — install it and come back.
                        Some(pending) => {
                            info!(version = %pending.version, "Restart to update clicked");
                            if let Some(mut listener) = input_listener.take() {
                                listener.stop();
                            }
                            apply_now(pending, true);
                            *control_flow = ControlFlow::Exit;
                        }
                        // Nothing staged — the user is asking "well, is
                        // there one?". Wake the worker; it reports back
                        // through UserEvent::Update like any other check.
                        None => {
                            info!("manual update check");
                            // `try_send` on a bounded(1): if a check is
                            // already queued, a second click is a no-op
                            // rather than a second round-trip.
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
                    // First-run: the directory typically doesn't
                    // exist yet — ensure_user_wordlist_dir creates
                    // it (and seeds a tiny README so the user knows
                    // what files are recognised) before we open it.
                    match ensure_user_wordlist_dir() {
                        Ok(dir) => open_path(&dir, "user wordlists folder"),
                        Err(e) => warn!(?e, "could not prepare user wordlists folder"),
                    }
                } else if id == layouts_id {
                    // Same first-run treatment as wordlists: ensure
                    // the directory exists and drop a README that
                    // explains the TOML schema so the user can copy
                    // an embedded mapping from the repo as a starting
                    // point. New layouts in this folder are picked up
                    // on app restart.
                    match ensure_user_layout_dir() {
                        Ok(dir) => open_path(&dir, "user layouts folder"),
                        Err(e) => warn!(?e, "could not prepare user layouts folder"),
                    }
                } else if id == reload_id {
                    // Reload `config.toml` AND re-read user-overlay
                    // wordlists (`<config-dir>/wordlists/<stem>.txt`).
                    // The latter is what lets users add tech vocab
                    // like `kubectl` / `terraform` and have it pick
                    // up without restarting the app.
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
                    // Pinned to `main`: the guide must reflect the
                    // latest setup script, not the binary that failed.
                    if let Err(e) = opener::open_browser(SETUP_GUIDE_URL) {
                        warn!(?e, "could not open the setup guide");
                        spawn_error_notification(format!(
                            "Could not open the setup guide.\nSee {SETUP_GUIDE_URL}"
                        ));
                    }
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
                SwitcherEvent::AddToDictionary { layout, word } => {
                    if let Err(e) = add_word_to_user_overlay(&layout, &word, &dict_reload_handle) {
                        warn!(?e, "could not add the word to the user wordlist overlay");
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
                        // Announce it once, on the transition. The worker
                        // only stages a given version once, so a user
                        // sitting on 0.4.0 for a week is told about it on
                        // the day it lands and never nagged again.
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
                    // Already logged by the worker. The tray entry stays
                    // as it was: a failed check is not news, and a user
                    // who has an update staged shouldn't lose the button
                    // to install it just because the *next* check
                    // couldn't reach GitHub.
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

/// CLI help text. Kept short and stable — most users never invoke
/// poltertype with arguments, but `--help` should still answer the
/// "what does this thing do" question without a manpage.
fn print_help() {
    println!(
        "{APP_NAME} {ver}\n\
        \n\
        USAGE:\n  \
            poltertype              start the tray app\n  \
            poltertype --settings   open the settings window\n  \
            poltertype --version    print version and exit\n  \
            poltertype --help       show this help",
        ver = env!("CARGO_PKG_VERSION"),
    );
}

/// Init `tracing` with both a stderr layer and a file appender that
/// rotates daily under `<data_dir>/poltertype/logs/`. Returns the
/// guard for the file writer; dropping it would close the file.
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

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

// ─── Noop key emitter (graceful fallback on unimplemented platforms) ──
