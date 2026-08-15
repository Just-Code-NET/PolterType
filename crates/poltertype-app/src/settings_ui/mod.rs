//! Iced-based Settings window, run as its own process
//! (`poltertype --settings`) because the tray already owns the
//! platform main thread on macOS. The two share nothing but
//! `config.toml`; the named pane is the whole protocol between them.
//!
//! See `docs/ARCHITECTURE.md` § Settings UI for the reasoning and for
//! the three ordering constraints this module has to respect.

mod consts;
mod enums;

pub use enums::Pane;
mod helpers;
mod plugin_pane;
mod state;
mod system_theme;
mod theme;
mod update;
mod view;
mod view_plugins;
mod view_setup;

use std::sync::Arc;

use anyhow::{Context, Result};
use poltertype_core::settings::SettingsStore;
use poltertype_layout::LayoutId;
use poltertype_layout::create_switcher;
use tracing::warn;

use state::*;

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
        Ok(dir) => poltertype_core::i18n::init(&dir, Some(&initial_settings.general.ui_language)),
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
    let app = iced::application(SettingsApp::title, SettingsApp::update, SettingsApp::view)
        .theme(SettingsApp::theme)
        .subscription(SettingsApp::subscription)
        .exit_on_close_request(false)
        .window(iced::window::Settings {
            // Sized so Commands and Wordlists render without scrolling
            // on 1080p; anything smaller clips their forms.
            size: iced::Size::new(860.0, 680.0),
            position: iced::window::Position::Centered,
            icon: helpers::window_icon(),
            ..Default::default()
        });

    let store_for_init = Arc::clone(&store);
    app.run_with(move || {
        let mut app = SettingsApp::new(
            initial_settings,
            os_layouts,
            path,
            store_for_init,
            initial,
            layout_backend,
        );
        // The pane the window opens on never fires its selection
        // handler, so its plug-in queries start here or never.
        let first = app.startup_task();
        (app, first)
    })
    .map_err(|e| anyhow::anyhow!("iced runtime: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests;
