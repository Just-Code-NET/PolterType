//! Iced-based Settings window for poltertype.
//!
//! ## Why a separate process
//!
//! The tray (`tao::EventLoop` + `tray-icon`) and `iced` both want to
//! own the platform's main thread on macOS — `NSApplication` is a
//! singleton and the tray already binds it. Rather than choreograph
//! a thread swap, we ship the Settings UI as a CLI subcommand:
//!
//!     poltertype --settings
//!
//! The tray spawns this as a child process when the user clicks
//! "Settings…". The two have nothing to share at runtime — the
//! settings UI just reads / writes `config.toml` on disk; when it
//! exits, the tray sees `SettingsReloaded` and refreshes its caches.
//!
//! The subprocess approach is not just a workaround:
//!
//! * Crashes in the UI never bring down the engine / hook.
//! * macOS / Windows / Linux behave identically — no per-platform
//!   thread juggling.
//! * The process boundary makes it trivial to run the UI on a
//!   different thread, in a debugger, or from a unit test driver.
//!
//! ## Sections
//!
//! Side-nav with six panes:
//!
//! * **Languages** — checkboxes for every layout the OS reports as
//!   active (queried via `LayoutSwitcher::list_active`). Toggling a
//!   box updates the `[languages].active` allow-list. An **empty**
//!   allow-list means "use every OS-active layout" — the default,
//!   and the UI displays it that way (every box ticked).
//! * **Hotkeys** — current pause / switch-last bindings, plus a
//!   "Rebind" button per row that flips the UI into capture mode
//!   and writes the next valid `<modifier>+<key>` combo back.
//! * **Wordlists** — multiline editor for the user-side wordlist
//!   overlays in `<config-dir>/poltertype/wordlists/<stem>.txt`
//!   (and `<stem>-stop.txt`). Pick a layout, pick the file kind,
//!   edit, then either click the unified footer Save or just
//!   close the window — both trigger a flush of any unsaved
//!   editor content. The tray's settings-waiter rebuilds the
//!   engine's dictionary set (and the per-profile cache) before
//!   sending `SettingsReloaded`, so edits apply without a tray
//!   restart.
//! * **General** — the boolean / numeric knobs from
//!   `GeneralSettings` + `EngineSettings`: autostart, sound on
//!   correction, suppress-in-identifiers, idle timeout.
//! * **Exceptions** — the per-app skip list (`disabled_apps`). One
//!   row per entry with a delete button; an "Add" row at the bottom
//!   for new entries.
//! * **About** — version + repo links. The bottom row also exposes
//!   a "Reset to defaults" button as a power-user escape hatch.

mod enums;
mod helpers;
mod state;
mod update;
mod view;

use std::sync::Arc;

use anyhow::{Context, Result};
use iced::Task;
use poltertype_core::settings::SettingsStore;
use poltertype_layout::LayoutId;
use poltertype_layout::create_switcher;
use tracing::warn;

use state::*;

/// Entry point: load settings + OS layouts, run iced loop, save on
/// "Save" click. Returns when the user closes the window.
pub fn run() -> Result<()> {
    let store = SettingsStore::load_or_default().context("load settings for UI")?;
    let initial_settings = store.snapshot();
    let store = Arc::new(store);

    // Querying the OS layout list is best-effort — if it fails we
    // present the user with an empty set and a hint, rather than
    // refusing to open the window. They can still edit other panes
    // and save.
    let os_layouts: Vec<LayoutId> = match create_switcher() {
        Ok(switcher) => switcher.list_active().unwrap_or_else(|e| {
            warn!(
                ?e,
                "could not list active OS layouts; Languages pane will be empty"
            );
            Vec::new()
        }),
        Err(e) => {
            warn!(
                ?e,
                "no layout switcher backend; Languages pane will be empty"
            );
            Vec::new()
        }
    };

    let path = store.path().to_path_buf();

    // iced::application requires `&'static str` OR a closure returning
    // `String` for the title. The closure form lets us include the
    // resolved config path so the user sees exactly which file the
    // window is editing — useful when running multiple installs side
    // by side or under a non-default `XDG_CONFIG_HOME`.
    // `exit_on_close_request(false)` lets us intercept the
    // window-close request, flush any unsaved wordlist edit to
    // disk, then close manually. Without this, a user who typed a
    // word in the Wordlists pane and clicked the window's close
    // button (instead of the per-pane Save) would lose the edit
    // silently — the bug report that prompted this fix.
    let app = iced::application(SettingsApp::title, SettingsApp::update, SettingsApp::view)
        .theme(SettingsApp::theme)
        .subscription(SettingsApp::subscription)
        .exit_on_close_request(false)
        // Window size was 720x540 in beta.11 and earlier — too cramped
        // for the Commands and Wordlists panes (Commands has a 6-row
        // add-form plus the existing list, Wordlists has a 260px-tall
        // editor + 3 picker rows + a path-hint line + tip). Bumped
        // here so the default render fits without scrolling on a
        // standard 1080p screen. Still small enough to feel like a
        // settings dialog, not a main window.
        .window_size((820.0, 640.0))
        .centered();

    let store_for_init = Arc::clone(&store);
    app.run_with(move || {
        (
            SettingsApp::new(initial_settings, os_layouts, path, store_for_init),
            Task::none(),
        )
    })
    .map_err(|e| anyhow::anyhow!("iced runtime: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests;
