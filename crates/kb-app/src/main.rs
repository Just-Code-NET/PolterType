//! kb-switcher application entry point.
//!
//! Phase 4 scaffold: tray, global keyboard listener, layout switcher,
//! `SwitcherEngine`, global hotkeys, file logging, and the
//! Open-Settings / Open-Logs / Reload-Settings tray entries. Full
//! visual GUI is deferred to Phase 8 (see `docs/DECISIONS.md`).

#![forbid(unsafe_code)]

mod icon_render;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, bounded, unbounded};
use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use kb_core::audio::AudioPlayer;
use kb_core::engine::{EngineCommand, SwitcherEngine, SwitcherEvent};
use kb_core::layouts::LayoutDb;
use kb_core::settings::SettingsStore;
use kb_detect::{Detector, DictionaryDetector, WordPlausibilityDetector};
use kb_input::{KeyEvent, create_emitter, create_focus_tracker, create_listener};
use kb_layout::create_switcher;
use kb_types::LayoutId;
use single_instance::SingleInstance;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tracing::{debug, error, info, warn};
use tray_icon::TrayIcon;
use tray_icon::TrayIconBuilder;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};

const APP_ID: &str = "dev.opensource.kb-switcher";
const APP_NAME: &str = "kb-switcher";

#[derive(Debug, Clone)]
enum UserEvent {
    Menu(MenuId),
    Hotkey(u32),
    Engine(SwitcherEvent),
}

fn main() -> Result<()> {
    let _log_guard = init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "{APP_NAME} starting");

    let instance = SingleInstance::new(APP_ID).context("create single-instance lock")?;
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

    // ─── Layouts ───────────────────────────────────────────────────
    // Embedded mappings + dictionaries baked at build time, plus
    // optional user overlay from `<config-dir>/wordlists/<stem>.txt`.
    let user_wordlist_dir = kb_core::layouts::user_wordlist_dir();
    let layouts = Arc::new(LayoutDb::load_embedded_with_user_overlay(
        user_wordlist_dir.as_deref(),
    ));
    info!(
        loaded = layouts.len(),
        ids = ?layouts.ids().collect::<Vec<_>>(),
        wordlist_overlay = ?user_wordlist_dir,
        "layout DB ready"
    );

    // ─── Subsystems ────────────────────────────────────────────────
    let layout_switcher = match create_switcher() {
        Ok(s) => {
            info!(backend = s.backend_name(), "layout switcher ready");
            Arc::from(s)
        }
        Err(e) => {
            error!(?e, "no layout switcher backend; aborting");
            return Err(anyhow::anyhow!(e));
        }
    };
    let key_emitter = match create_emitter() {
        Ok(e) => {
            info!(backend = e.backend_name(), "key emitter ready");
            Arc::from(e)
        }
        Err(e) => {
            warn!(?e, "no key emitter backend; corrections will be no-op");
            Arc::from(noop_emitter()) as Arc<dyn kb_input::KeyEmitter>
        }
    };
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
    // (re-reading user-overlay files) without restarting.
    let dict_reload_handle = dictionary.handle();
    let detectors: Vec<Box<dyn Detector>> = vec![
        Box::new(dictionary),
        Box::new(build_plausibility_detector(&layouts)),
    ];

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
        Arc::clone(&focus_tracker),
        Arc::clone(&audio),
        engine_event_tx,
    );
    std::thread::Builder::new()
        .name("kb-switcher-engine".into())
        .spawn(move || engine.run(key_rx, engine_cmd_rx))
        .context("spawn engine thread")?;

    // ─── Input listener ────────────────────────────────────────────
    let mut input_listener = create_listener().ok();
    if let Some(listener) = input_listener.as_mut() {
        if let Err(e) = listener.start(key_tx) {
            warn!(?e, "input listener failed to start");
        } else {
            info!(backend = listener.backend_name(), "input listener started");
        }
    } else {
        warn!("no input listener backend; engine will receive no events");
    }

    // ─── Tao event loop + tray + global hotkeys ────────────────────
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let menu = Menu::new();
    let item_settings = MenuItem::new("Open Settings (config.toml)…", true, None);
    let item_logs = MenuItem::new("Open Logs Folder…", true, None);
    let item_wordlists = MenuItem::new("Open User Wordlists Folder…", true, None);
    let item_reload = MenuItem::new("Reload Settings", true, None);
    let item_pause = MenuItem::new("Pause auto-switch", true, None);
    let item_about = MenuItem::new(
        format!("About {APP_NAME} v{}", env!("CARGO_PKG_VERSION")),
        false,
        None,
    );
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[
        &item_settings,
        &item_logs,
        &item_wordlists,
        &item_reload,
        &PredefinedMenuItem::separator(),
        &item_pause,
        &PredefinedMenuItem::separator(),
        &item_about,
        &item_quit,
    ])
    .context("populate tray menu")?;
    let settings_id = item_settings.id().clone();
    let logs_id = item_logs.id().clone();
    let wordlists_id = item_wordlists.id().clone();
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

    let tray: TrayIcon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip_for(initial_layout.as_ref(), false))
        .with_icon(initial_icon)
        .build()
        .context("build tray icon")?;

    // Cloned reference into the event loop so we can flip the menu
    // item's text between "⏸ Pause auto-switch" and "▶ Resume
    // auto-switch" when the engine reports a state change. MenuItem
    // is internally Arc-shared, so this clone bumps a refcount.
    let item_pause_for_loop = item_pause.clone();

    // Global hotkeys
    let hotkey_manager = GlobalHotKeyManager::new().context("create global-hotkey manager")?;
    let hk_pause = HotKey::new(Some(HkMods::CONTROL | HkMods::SHIFT), Code::Space);
    let hk_switch = HotKey::new(Some(HkMods::CONTROL | HkMods::SHIFT), Code::Backspace);
    if let Err(e) = hotkey_manager.register(hk_pause) {
        warn!(?e, "could not register pause hotkey (Ctrl+Shift+Space)");
    }
    if let Err(e) = hotkey_manager.register(hk_switch) {
        warn!(
            ?e,
            "could not register switch-last hotkey (Ctrl+Shift+Backspace)"
        );
    }
    let pause_hotkey_id = hk_pause.id();
    let switch_hotkey_id = hk_switch.id();

    spawn_event_bridges(event_loop.create_proxy(), engine_event_rx.clone())?;

    // Layout poller: the engine emits LayoutChanged for switches it
    // performs itself, but we miss user-driven manual switches (Win+
    // Space / Alt+Shift / language bar / ibus / kde-keyboard). Polling
    // the OS-level current-layout query every ~250 ms catches those
    // cheaply and keeps the tray icon in sync.
    spawn_layout_poller(Arc::clone(&layout_switcher), engine_event_tx_for_poller)?;

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
                    *control_flow = ControlFlow::Exit;
                } else if id == settings_id {
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
                }
            }
            Event::UserEvent(UserEvent::Hotkey(id)) => {
                if id == pause_hotkey_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::TogglePause);
                } else if id == switch_hotkey_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::SwitchLastForcefully);
                }
            }
            Event::UserEvent(UserEvent::Engine(ev)) => {
                handle_engine_event(ev, &tray, &item_pause_for_loop, &mut tray_state);
            }
            _ => {}
        }
    });
}

/// Snapshot of "what should the tray look like right now". We need
/// both fields to render the icon / tooltip correctly — paused state
/// affects styling regardless of layout, and vice versa — so we
/// redraw from the whole struct on every relevant event.
struct TrayState {
    layout: Option<LayoutId>,
    paused: bool,
}

fn tooltip_for(layout: Option<&LayoutId>, paused: bool) -> String {
    match (layout, paused) {
        (Some(l), false) => format!("{APP_NAME} — {l}"),
        (Some(l), true) => format!("{APP_NAME} — {l} (paused)"),
        (None, false) => APP_NAME.to_owned(),
        (None, true) => format!("{APP_NAME} (paused)"),
    }
}

/// Redraw icon + tooltip + the pause menu-item text from the current
/// `TrayState`. Cheap (no allocation in the icon-rendering path beyond
/// a 16x16 RGBA buffer); safe to call on every state change.
fn refresh_tray(tray: &TrayIcon, item_pause: &MenuItem, state: &TrayState) {
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
    if let Err(e) = tray.set_tooltip(Some(tooltip_for(state.layout.as_ref(), state.paused))) {
        warn!(?e, "could not update tray tooltip");
    }
    item_pause.set_text(if state.paused {
        "▶ Resume auto-switch"
    } else {
        "⏸ Pause auto-switch"
    });
}

fn open_path(path: &std::path::Path, what: &str) {
    debug!(?path, "opening {what}");
    if let Err(e) = opener::open(path) {
        warn!(?e, ?path, "could not open {what} in default app");
    }
}

/// Resolve the user wordlists directory, create it if missing, and
/// drop a `README.txt` on first creation so a user opening the
/// folder for the first time can immediately see what files are
/// recognised. Returns the directory path on success.
///
/// We seed only on actual creation — once the user has the folder,
/// we never touch the README again, so users can delete it / rename
/// it / replace it without our re-overwriting their changes.
fn ensure_user_wordlist_dir() -> anyhow::Result<PathBuf> {
    let dir = kb_core::layouts::user_wordlist_dir()
        .context("could not determine user-config directory")?;
    let needs_seed = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create wordlists dir at {}", dir.display()))?;
    if needs_seed {
        let readme = dir.join("README.txt");
        // Best-effort write — failure is logged but doesn't block
        // opening the folder. The directory itself is the value.
        if let Err(e) = std::fs::write(&readme, USER_WORDLISTS_README) {
            warn!(?e, ?readme, "could not seed README in wordlists folder");
        }
    }
    Ok(dir)
}

/// One-time README seeded into the user wordlists folder. Plain
/// text (no markdown), short, and readable in any editor / preview
/// pane. Matches the file conventions documented in
/// `kb_core::layouts::build_dictionary`.
const USER_WORDLISTS_README: &str = "\
kb-switcher — user wordlists
=============================

Drop text files here to extend the built-in dictionaries without
rebuilding the app. Changes are picked up on the next \"Reload
Settings\" tray click (Ctrl+Shift+R if you've bound it) — no restart
needed.

Per layout, three filenames are recognised. Replace `<stem>` with the
layout id you want to extend (`en_us`, `uk_ua`, …):

    <stem>.txt          One word per line; treated as a real word
                        in this layout, regardless of length.
                        Use this for tech vocab, surnames, slang,
                        product names — anything that should NOT
                        get auto-corrected away.

    <stem>-extras.txt   Same effect as <stem>.txt; separate file
                        so you can organise (e.g. one for tech
                        vocab, one for personal names). Both are
                        merged into the same overlay at load time.

    <stem>-stop.txt     Curated 1- and 2-letter additions. Needed
                        when you want a SHORT (≤2 letter) token
                        treated as a real word — at that length
                        the embedded full dictionary is bypassed
                        on purpose, so this is the only path that
                        works for short tokens.

Format for all three:
    - one lowercase word per line
    - blank lines and `# comment` lines ignored
    - UTF-8

Example (`uk_ua.txt`):
    кубернетес
    докерфайл
    редіс

Example (`uk_ua-stop.txt`):
    хм
    тю

Tip: the embedded dictionaries already cover ~370k EN and ~333k UK
entries plus a curated tech-vocab list. You only need files here for
words you actually see auto-corrected wrongly.
";

fn handle_engine_event(
    ev: SwitcherEvent,
    tray: &TrayIcon,
    item_pause: &MenuItem,
    state: &mut TrayState,
) {
    match ev {
        SwitcherEvent::Corrected {
            from_layout,
            to_layout,
            original_text,
            corrected_text,
            reason,
        } => {
            info!(
                %from_layout,
                %to_layout,
                original = %original_text,
                corrected = %corrected_text,
                %reason,
                "correction applied"
            );
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
    }
}

fn spawn_event_bridges(
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

fn build_plausibility_detector(layouts: &Arc<LayoutDb>) -> WordPlausibilityDetector {
    let profiles = layouts
        .iter()
        .map(|(id, m)| (id.clone(), m.detector_profile()))
        .collect();
    WordPlausibilityDetector::new(profiles)
}

fn build_dictionary_detector(layouts: &Arc<LayoutDb>) -> DictionaryDetector {
    DictionaryDetector::new(collect_dicts(layouts))
}

fn collect_dicts(
    layouts: &LayoutDb,
) -> std::collections::HashMap<kb_types::LayoutId, kb_detect::LayoutDictionary> {
    layouts
        .iter()
        .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
        .collect()
}

/// Re-read `<config-dir>/kb-switcher/wordlists/<stem>.txt` from disk
/// and atomically swap the engine's dictionary set. Returns the
/// number of dictionaries successfully loaded. Always rebuilds — even
/// when the user added zero new entries — so the user gets a clear
/// signal in the log that the reload took effect.
fn reload_user_dictionaries(handle: &DictionaryDetector) -> usize {
    let user_dir = kb_core::layouts::user_wordlist_dir();
    let new_layouts = LayoutDb::load_embedded_with_user_overlay(user_dir.as_deref());
    let new_dicts = collect_dicts(&new_layouts);
    let n = new_dicts.len();
    handle.replace_dicts(new_dicts);
    info!(
        loaded = n,
        overlay_dir = ?user_dir,
        "user wordlist overlays reloaded"
    );
    n
}

/// Poll the OS for the current layout every ~250 ms; emit a
/// `LayoutChanged` event whenever the answer differs from last time.
/// Catches manual switches done outside our engine (language bar,
/// Win+Space, Alt+Shift, ibus / kde keyboard, …) so the tray icon
/// stays in sync.
fn spawn_layout_poller(
    switcher: Arc<dyn kb_layout::LayoutSwitcher>,
    out_tx: crossbeam_channel::Sender<SwitcherEvent>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("kb-layout-poller".into())
        .spawn(move || {
            let mut last: Option<LayoutId> = None;
            loop {
                if let Ok(current) = switcher.current() {
                    if last.as_ref() != Some(&current) {
                        if out_tx
                            .send(SwitcherEvent::LayoutChanged(current.clone()))
                            .is_err()
                        {
                            break;
                        }
                        last = Some(current);
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .context("spawn layout poller thread")?;
    Ok(())
}

/// Init `tracing` with both a stderr layer and a file appender that
/// rotates daily under `<data_dir>/kb-switcher/logs/`. Returns the
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
                eprintln!("kb-switcher: could not create log dir {dir:?}: {e}");
                (None, None)
            } else {
                let appender = tracing_appender::rolling::daily(&dir, "kb-switcher.log");
                let (writer, guard) = tracing_appender::non_blocking(appender);
                let layer = fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false)
                    .with_target(false);
                (Some(layer), Some(guard))
            }
        }
        Err(e) => {
            eprintln!("kb-switcher: cannot resolve log dir: {e}");
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

struct NoopEmitter;

impl kb_input::KeyEmitter for NoopEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), kb_input::InputError> {
        debug!(n, "noop emitter: would send backspaces");
        Ok(())
    }
    fn send_text(&self, text: &str) -> Result<(), kb_input::InputError> {
        debug!(text, "noop emitter: would send text");
        Ok(())
    }
    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

fn noop_emitter() -> Box<dyn kb_input::KeyEmitter> {
    Box::new(NoopEmitter)
}
