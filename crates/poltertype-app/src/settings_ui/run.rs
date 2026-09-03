//! Boot the iced application: load settings, probe OS layouts, run the
//! event loop, and hand back once the window closes.

use std::sync::Arc;

use anyhow::{Context, Result};
use poltertype_core::settings::SettingsStore;
use poltertype_layout::LayoutId;
use poltertype_layout::create_switcher;
use tracing::warn;

use super::enums;
use super::helpers;
use super::state::SettingsApp;
use super::theme;

/// Entry point: load settings + OS layouts, run the iced loop, save on
/// "Save". Returns when the user closes the window.
///
/// `open_setup` starts on the **Setup** pane instead of Languages —
/// what the tray passes when the keyboard hooks failed to start.
pub fn run(open_setup: bool) -> Result<()> {
    run_on(if open_setup {
        enums::Pane::Setup
    } else {
        enums::Pane::Languages
    })
}

/// Open the window on a named pane. `--setup` and `--plugins` both
/// come through here.
pub fn run_on(initial: enums::Pane) -> Result<()> {
    let store = SettingsStore::load_or_default().context("load settings for UI")?;
    let initial_settings = store.snapshot();

    // Must precede the first widget: `tr` runs from the view function,
    // on every frame. No catalog is not an error — English call sites.
    match poltertype_core::data_dir::resolve() {
        Ok(dir) => {
            // Also before the panes are built: a plug-in's manifest is
            // translated the moment it is loaded into one.
            let plugins = poltertype_core::plugins::catalog_sources(&dir);
            poltertype_core::i18n::init(
                &dir,
                Some(&initial_settings.general.ui_language),
                &plugins,
            );
        }
        Err(e) => warn!(?e, "no data dir; the interface stays in English"),
    }

    let store = Arc::new(store);

    // Best-effort: a failure yields an empty set and a hint rather
    // than refusing to open the window. The same probe tells the Setup
    // pane whether a layout switcher exists at all.
    let (os_layouts, layout_backend): (Vec<LayoutId>, Option<String>) = match create_switcher() {
        Ok(switcher) => {
            let backend = switcher.backend_name().to_owned();
            let layouts = switcher.list_active().unwrap_or_else(|e| {
                warn!(
                    ?e,
                    "could not list active OS layouts; Languages pane will be empty"
                );
                Vec::new()
            });
            (layouts, Some(backend))
        }
        Err(e) => {
            warn!(
                ?e,
                "no layout switcher backend; Languages pane will be empty"
            );
            (Vec::new(), None)
        }
    };

    let path = store.path().to_path_buf();

    // `exit_on_close_request(false)` is load-bearing: the window
    // intercepts the close request to flush an unsaved wordlist edit
    // before closing. See `docs/ARCHITECTURE.md` § Settings UI.
    let store_for_init = Arc::clone(&store);
    // iced 0.14 takes the boot function where 0.13 took the title, and
    // `run_with` folded into `run` once boot was a constructor
    // argument. Same two jobs, in the other order.
    //
    // Boot has to be `Fn` rather than `FnOnce`, so the closure clones
    // what it hands over rather than moving it. iced calls it once and
    // the payload is a handful of strings and an `Arc`; a cell holding
    // it for a single take would buy nothing but a panic path.
    let app = iced::application(
        move || {
            let mut app = SettingsApp::new(
                initial_settings.clone(),
                os_layouts.clone(),
                path.clone(),
                Arc::clone(&store_for_init),
                initial,
                layout_backend.clone(),
            );
            // The pane the window opens on never fires its selection
            // handler, so its plug-in queries start here or never.
            let first = app.startup_task();
            (app, first)
        },
        SettingsApp::update,
        SettingsApp::view,
    )
    .title(SettingsApp::title)
    .theme(SettingsApp::theme)
        .subscription(SettingsApp::subscription)
        // Every label that names no font of its own. Left at iced's
        // default this asks for a family most machines do not have —
        // see `theme::font_ui`.
        .default_font(theme::font_ui())
        .exit_on_close_request(false)
        .window(iced::window::Settings {
            // Sized so Commands and Wordlists render without scrolling
            // on 1080p; anything smaller clips their forms.
            size: iced::Size::new(860.0, 680.0),
            // An UNVERIFIED mitigation, never a fix: a sudden large
            // resize can hit an iced_tiny_skia 0.13 `debug_assert!` —
            // some quad's height lands on exactly 0.0 for one frame —
            // and the window dies (`Quad with non-normal height!`,
            // engine.rs:43). Debug builds only. Measured 2026-08-19: a
            // compositor-driven resize sails straight past this floor
            // and crashed at 300×300 with it set; xdg-shell min-size
            // binds only the compositor's own interactive grab, so a
            // surviving mouse-drag test would prove nothing either.
            min_size: Some(iced::Size::new(480.0, 420.0)),
            position: iced::window::Position::Centered,
            icon: helpers::window_icon(),
            // Left at its default this is the empty string, which on
            // Linux means "no application" rather than "unset" — and
            // the desktop entry it names is the only place a Wayland
            // session finds a window icon.
            platform_specific: poltertype_shell::window_platform_specific(),
            ..Default::default()
        });

    app.run()
        .map_err(|e| anyhow::anyhow!("iced runtime: {e}"))?;

    Ok(())
}
