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

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, bounded, unbounded};
use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use kb_core::audio::AudioPlayer;
use kb_core::engine::{EngineCommand, SwitcherEngine, SwitcherEvent};
use kb_core::layouts::LayoutDb;
use kb_core::settings::SettingsStore;
use kb_detect::{Detector, WordPlausibilityDetector};
use kb_input::{KeyEvent, create_emitter, create_listener};
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
    let layouts = Arc::new(LayoutDb::load_embedded());
    info!(loaded = layouts.len(), ids = ?layouts.ids().collect::<Vec<_>>(), "layout DB ready");

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

    let detectors: Vec<Box<dyn Detector>> = vec![Box::new(build_plausibility_detector(&layouts))];

    // ─── Engine ────────────────────────────────────────────────────
    let (key_tx, key_rx) = bounded::<KeyEvent>(1024);
    let (engine_event_tx, engine_event_rx) = unbounded::<SwitcherEvent>();
    let (engine_cmd_tx, engine_cmd_rx) = unbounded::<EngineCommand>();

    let engine = SwitcherEngine::new(
        Arc::clone(&settings),
        Arc::clone(&layouts),
        detectors,
        Arc::clone(&layout_switcher),
        Arc::clone(&key_emitter),
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
    let reload_id = item_reload.id().clone();
    let pause_id = item_pause.id().clone();
    let quit_id = item_quit.id().clone();

    // Initial icon: query the OS for the current layout so we don't
    // flash a "??" before the first LayoutChanged event arrives.
    let initial_layout: Option<LayoutId> = layout_switcher.current().ok();
    let initial_icon = match initial_layout.as_ref() {
        Some(l) => icon_render::for_layout(l)?,
        None => icon_render::unknown()?,
    };

    let tray: TrayIcon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(initial_tooltip(initial_layout.as_ref()))
        .with_icon(initial_icon)
        .build()
        .context("build tray icon")?;

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

    let settings_path: PathBuf = settings.path().to_owned();
    let log_dir: Option<PathBuf> = SettingsStore::log_dir().ok();
    let cmd_tx_for_loop = engine_cmd_tx.clone();
    let settings_for_loop = Arc::clone(&settings);

    info!("entering event loop");
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Menu(id)) => {
                let _ = &tray; // keep alive; touched in match arms below
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
                } else if id == reload_id {
                    match settings_for_loop.reload() {
                        Ok(true) => {
                            info!("config.toml reloaded — settings changed");
                            let _ = cmd_tx_for_loop.send(EngineCommand::SettingsReloaded);
                        }
                        Ok(false) => info!("config.toml reloaded — no changes"),
                        Err(e) => warn!(?e, "could not reload settings"),
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
            Event::UserEvent(UserEvent::Engine(ev)) => handle_engine_event(ev, &tray),
            _ => {}
        }
    });
}

fn initial_tooltip(layout: Option<&LayoutId>) -> String {
    match layout {
        Some(l) => format!("{APP_NAME} — {l}"),
        None => APP_NAME.to_owned(),
    }
}

fn update_tray_for_layout(tray: &TrayIcon, layout: &LayoutId) {
    match icon_render::for_layout(layout) {
        Ok(icon) => {
            if let Err(e) = tray.set_icon(Some(icon)) {
                warn!(?e, layout = %layout, "could not update tray icon");
            }
        }
        Err(e) => warn!(?e, layout = %layout, "could not render tray icon"),
    }
    if let Err(e) = tray.set_tooltip(Some(format!("{APP_NAME} — {layout}"))) {
        warn!(?e, "could not update tray tooltip");
    }
}

fn open_path(path: &std::path::Path, what: &str) {
    debug!(?path, "opening {what}");
    if let Err(e) = opener::open(path) {
        warn!(?e, ?path, "could not open {what} in default app");
    }
}

fn handle_engine_event(ev: SwitcherEvent, tray: &TrayIcon) {
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
        }
        SwitcherEvent::LayoutChanged(id) => {
            debug!(layout = %id, "layout changed");
            update_tray_for_layout(tray, &id);
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
