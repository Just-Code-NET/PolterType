//! kb-switcher application entry point.
//!
//! Phase 3 scaffold: tray, global keyboard listener, layout switcher,
//! `SwitcherEngine`, and global hotkeys (pause / switch-last).
//! Settings UI lands in Phase 4.

#![forbid(unsafe_code)]

use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use kb_core::audio::AudioPlayer;
use kb_core::engine::{EngineCommand, SwitcherEngine, SwitcherEvent};
use kb_core::layouts::LayoutDb;
use kb_core::settings::SettingsStore;
use kb_detect::{Detector, WordPlausibilityDetector};
use kb_input::{KeyEvent, create_emitter, create_listener};
use kb_layout::create_switcher;
use single_instance::SingleInstance;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tracing::{debug, error, info, warn};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

const APP_ID: &str = "dev.opensource.kb-switcher";
const APP_NAME: &str = "kb-switcher";

#[derive(Debug, Clone)]
enum UserEvent {
    Menu(MenuId),
    Hotkey(u32),
    Engine(SwitcherEvent),
}

fn main() -> Result<()> {
    init_tracing();
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
    if layouts.is_empty() {
        warn!("no layouts loaded — engine will be a no-op until at least two layouts exist");
    }

    // ─── Layout switcher ───────────────────────────────────────────
    let layout_switcher = match create_switcher() {
        Ok(s) => {
            info!(backend = s.backend_name(), "layout switcher ready");
            Arc::from(s)
        }
        Err(e) => {
            error!(?e, "no layout switcher backend on this platform; aborting");
            return Err(anyhow::anyhow!(e));
        }
    };

    // ─── Key emitter ───────────────────────────────────────────────
    let key_emitter = match create_emitter() {
        Ok(e) => {
            info!(backend = e.backend_name(), "key emitter ready");
            Arc::from(e)
        }
        Err(e) => {
            warn!(
                ?e,
                "no key emitter backend; corrections will be DECISION-ONLY"
            );
            // Stub-friendly: build a noop emitter so the engine can
            // still log decisions on platforms without an impl yet.
            Arc::from(noop_emitter()) as Arc<dyn kb_input::KeyEmitter>
        }
    };

    // ─── Audio ─────────────────────────────────────────────────────
    let audio = Arc::new(AudioPlayer::new());
    audio.refresh_from(&settings);

    // ─── Detectors ─────────────────────────────────────────────────
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
    let mut input_listener = match create_listener() {
        Ok(l) => Some(l),
        Err(e) => {
            warn!(
                ?e,
                "no input listener backend; engine will receive no events"
            );
            None
        }
    };
    if let Some(listener) = input_listener.as_mut() {
        if let Err(e) = listener.start(key_tx) {
            warn!(?e, "input listener failed to start");
        } else {
            info!(backend = listener.backend_name(), "input listener started");
        }
    }

    // ─── Tao event loop + tray + global hotkeys ────────────────────
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let menu = Menu::new();
    let item_settings = MenuItem::new("Settings…", true, None);
    let item_pause = MenuItem::new("Pause auto-switch", true, None);
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[
        &item_settings,
        &item_pause,
        &PredefinedMenuItem::separator(),
        &item_quit,
    ])
    .context("populate tray menu")?;
    let settings_id = item_settings.id().clone();
    let pause_id = item_pause.id().clone();
    let quit_id = item_quit.id().clone();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME)
        .with_icon(build_placeholder_icon()?)
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

    info!("entering event loop");
    let cmd_tx_for_loop = engine_cmd_tx.clone();
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
                    info!("Settings clicked (UI lands in Phase 4)");
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
                handle_engine_event(ev);
            }
            _ => {}
        }
    });
}

fn handle_engine_event(ev: SwitcherEvent) {
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
    // Tray menu → loop
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

    // Global hotkeys → loop
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

    // Engine events → loop
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

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).with_target(false).init();
}

fn build_placeholder_icon() -> Result<Icon> {
    const W: u32 = 16;
    const H: u32 = 16;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..(W * H) {
        rgba.extend_from_slice(&[0x4F, 0x9D, 0xFF, 0xFF]);
    }
    Icon::from_rgba(rgba, W, H).context("build placeholder tray icon")
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

// Drop-channel guard so unused senders don't poison the workspace.
#[allow(dead_code)]
fn _ensure_sender_used(_t: Sender<KeyEvent>) {}
