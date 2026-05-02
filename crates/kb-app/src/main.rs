//! kb-switcher application entry point.
//!
//! Phase 2 scaffold: tray + global keyboard listener + layout switcher.
//! Settings UI lands in Phase 4; engine in Phase 3.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, bounded};
use single_instance::SingleInstance;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tracing::{debug, error, info, warn};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use kb_input::{KeyEvent, create_listener};
use kb_layout::{LayoutSwitcher, create_switcher};

const APP_ID: &str = "dev.opensource.kb-switcher";
const APP_NAME: &str = "kb-switcher";

#[derive(Debug, Clone)]
enum UserEvent {
    Menu(MenuId),
}

fn main() -> Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "{APP_NAME} starting");

    let instance = SingleInstance::new(APP_ID).context("create single-instance lock")?;
    if !instance.is_single() {
        warn!("another instance of {APP_NAME} is already running, exiting");
        return Ok(());
    }

    // ---- Layout switcher: query the OS so we have a baseline -----------
    let layout_switcher = match create_switcher() {
        Ok(sw) => {
            info!(backend = sw.backend_name(), "layout switcher ready");
            match sw.current() {
                Ok(id) => info!(layout = %id, "current layout"),
                Err(e) => warn!(?e, "could not query current layout"),
            }
            match sw.list_active() {
                Ok(list) => info!(?list, "active layouts"),
                Err(e) => warn!(?e, "could not list active layouts"),
            }
            Some(sw)
        }
        Err(e) => {
            warn!(?e, "no layout switcher backend available on this platform");
            None
        }
    };
    let _layout_switcher: Option<Box<dyn LayoutSwitcher>> = layout_switcher;

    // ---- Input listener: install OS hook --------------------------------
    let (key_tx, key_rx) = bounded::<KeyEvent>(1024);
    let mut input_listener = match create_listener() {
        Ok(l) => Some(l),
        Err(e) => {
            warn!(?e, "no input listener backend available on this platform");
            None
        }
    };
    if let Some(listener) = input_listener.as_mut() {
        match listener.start(key_tx) {
            Ok(()) => info!(backend = listener.backend_name(), "input listener started"),
            Err(e) => warn!(?e, "input listener failed to start"),
        }
    }

    spawn_key_drain(key_rx).context("spawn key-event drain thread")?;

    // ---- Tray ----------------------------------------------------------
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let menu = Menu::new();
    let item_settings = MenuItem::new("Settings…", true, None);
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[&item_settings, &PredefinedMenuItem::separator(), &item_quit])
        .context("populate tray menu")?;

    let settings_id = item_settings.id().clone();
    let quit_id = item_quit.id().clone();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME)
        .with_icon(build_placeholder_icon()?)
        .build()
        .context("build tray icon")?;

    let proxy = event_loop.create_proxy();
    std::thread::Builder::new()
        .name("tray-menu-bridge".into())
        .spawn(move || {
            let rx = MenuEvent::receiver();
            while let Ok(event) = rx.recv() {
                if proxy.send_event(UserEvent::Menu(event.id)).is_err() {
                    break;
                }
            }
        })
        .context("spawn menu bridge thread")?;

    info!("entering event loop");
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::UserEvent(UserEvent::Menu(id)) = event {
            if id == quit_id {
                info!("Quit clicked — shutting down");
                if let Some(mut listener) = input_listener.take() {
                    listener.stop();
                }
                *control_flow = ControlFlow::Exit;
            } else if id == settings_id {
                info!("Settings clicked (UI lands in Phase 4)");
            }
        }
    });
}

/// Drain key events on a worker thread; for Phase 2 we only log them.
/// Phase 3 will hand them to the SwitcherEngine instead.
fn spawn_key_drain(rx: Receiver<KeyEvent>) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("key-event-drain".into())
        .spawn(move || {
            let mut counted: u64 = 0;
            for ev in rx.iter() {
                counted += 1;
                if counted <= 8 || counted % 100 == 0 {
                    debug!(
                        n = counted,
                        vk = ev.vk,
                        sc = ev.scancode,
                        dir = ?ev.direction,
                        mods = ?ev.modifiers,
                        injected = ev.injected,
                        "key event"
                    );
                }
            }
            error!("key event channel closed");
        })
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
