//! kb-switcher application entry point.
//!
//! Phase 1 scaffold: single-instance check, tracing, `tao` event loop,
//! and a system tray with Settings/Quit menu items. No keyboard hooks
//! yet (Phase 2). No settings window yet (Phase 4).

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use single_instance::SingleInstance;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tracing::{info, warn};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

const APP_ID: &str = "dev.opensource.kb-switcher";
const APP_NAME: &str = "kb-switcher";

/// Events forwarded from background channels into tao's event loop.
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

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // ---- Build tray menu ----
    let menu = Menu::new();
    let item_settings = MenuItem::new("Settings…", true, None);
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[&item_settings, &PredefinedMenuItem::separator(), &item_quit])
        .context("populate tray menu")?;

    let settings_id = item_settings.id().clone();
    let quit_id = item_quit.id().clone();

    // Build tray icon. Keep the handle alive for the lifetime of the
    // event loop — dropping it would remove the icon from the tray.
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME)
        .with_icon(build_placeholder_icon()?)
        .build()
        .context("build tray icon")?;

    // Bridge tray-icon's mpsc receiver into tao's user-event channel.
    // tray-icon publishes menu clicks on a global crossbeam receiver;
    // tao polls events on the main thread, so we need this hop.
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
                info!("Quit clicked — exiting");
                *control_flow = ControlFlow::Exit;
            } else if id == settings_id {
                // Phase 4 will open the iced settings window here.
                info!("Settings clicked (UI not implemented yet)");
            }
        }
    });
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).with_target(false).init();
}

/// 16x16 RGBA placeholder until we ship real per-language tray glyphs.
fn build_placeholder_icon() -> Result<Icon> {
    const W: u32 = 16;
    const H: u32 = 16;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..(W * H) {
        rgba.extend_from_slice(&[0x4F, 0x9D, 0xFF, 0xFF]);
    }
    Icon::from_rgba(rgba, W, H).context("build placeholder tray icon")
}
