//! Spawning the Settings GUI child process and refreshing the engine
//! when it closes.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use poltertype_core::engine::EngineCommand;
use tracing::{info, warn};

use super::enums::*;
use super::exe::*;
use crate::bridges::spawn_error_notification;
use crate::detectors::*;
use crate::types::*;

/// Spawn the Settings GUI as a child process (`poltertype --settings`)
/// and refresh the engine when the window closes. Subprocess rather
/// than in-process for the reason in `docs/ARCHITECTURE.md`.
///
/// All three refreshes run on close, so every kind of edit takes effect
/// before focus returns to the user's app:
///
/// 1. **`config.toml` reload** — text triggers, hotkey rebindings,
///    exception list, profile schema.
/// 2. **Global wordlist reload** — re-read and swapped through
///    `DictionaryDetector::replace_dicts`.
/// 3. **Per-profile cache rebuild + force-reapply** — the watcher
///    otherwise only swaps on profile transitions, so a user editing
///    words while focused on a profiled app would see no effect until
///    they alt-tabbed away and back.
///
/// A failure to spawn gets a notification, not just a log line: the
/// click produced no window, and a tray app has nowhere else to put an
/// error.
pub(crate) fn spawn_settings_ui(deps: SettingsCloseDeps) {
    spawn_settings_ui_on(deps, SettingsEntry::Normal)
}

/// Same, but opening on the Setup pane — what the tray's "keyboard
/// hooks unavailable" alert now does instead of throwing the user at a
/// markdown file in a browser.
pub(crate) fn spawn_setup_ui(deps: SettingsCloseDeps) {
    spawn_settings_ui_on(deps, SettingsEntry::Setup)
}

fn spawn_settings_ui_on(deps: SettingsCloseDeps, entry: SettingsEntry) {
    let Some(exe) = settings_ui_exe() else {
        return;
    };
    info!(?exe, ?entry, "launching settings UI");
    let child = match std::process::Command::new(&exe).arg(entry.flag()).spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(?e, ?exe, "settings UI subprocess failed to start");
            spawn_error_notification(format!(
                "Couldn't open Settings: {e}.\nRestarting {app} should fix it.",
                app = crate::consts::APP_NAME,
            ));
            return;
        }
    };

    // Waited on in a worker thread so the tray does not block. All
    // three refresh steps run whether or not the user clicked Save: the
    // GUI also writes files outside its own state, so reload-on-close
    // is the predictable contract.
    std::thread::Builder::new()
        .name("poltertype-settings-waiter".into())
        .spawn(move || {
            let mut child = child;
            match child.wait() {
                Ok(status) => info!(?status, "settings UI exited"),
                Err(e) => warn!(?e, "could not wait on settings UI child"),
            }

            // (1) config.toml reload.
            match deps.settings.reload() {
                Ok(changed) => info!(changed, "config.toml reloaded after settings UI exit"),
                Err(e) => warn!(?e, "could not reload config.toml after settings UI exit"),
            }

            // (1a) The autostart checkbox edits config.toml like any
            // other setting; re-apply it to the OS now that the file
            // is re-read.
            poltertype_autostart::sync(
                deps.settings.snapshot().general.autostart,
                poltertype_autostart::App {
                    id: crate::consts::APP_ID,
                    name: crate::consts::APP_NAME,
                },
            );

            // (2) Global wordlist reload — same path as the tray
            // "Reload Settings" menu entry.
            let n = reload_user_dictionaries(&deps.dict_reload_handle);
            info!(
                loaded = n,
                "wordlist dictionaries reloaded after settings UI exit"
            );

            // (3) Profile cache rebuild + watcher force-reapply.
            // Rebuild always, even when the user has no profiles
            // configured — cheap (empty cache) and keeps the
            // contract uniform.
            let snap = deps.settings.snapshot();
            let fresh_cache = build_full_profile_cache(
                &deps.layouts,
                &deps.data_dir,
                &snap.wordlists,
                deps.user_wordlist_dir.as_deref(),
            );
            let n_profiles = fresh_cache.len();
            *deps.profile_dict_cache.write() = fresh_cache;
            deps.profile_force_reapply.store(true, Ordering::Release);
            info!(
                profiles = n_profiles,
                "profile cache rebuilt; watcher will re-apply on next tick"
            );

            // (4) Tell the engine to clear its word buffer + refresh
            // audio for the new settings snapshot. Sent last so any
            // observer sees the rebuilds before the engine command.
            if let Err(e) = deps.reload_tx.send(EngineCommand::SettingsReloaded) {
                warn!(?e, "could not enqueue SettingsReloaded after UI exit");
            }
        })
        .ok();
}

/// Which binary to hand to `Command::new`, or `None` when there is
/// nothing launchable — in which case the user has already been told.
///
/// The interesting case is a tray that has outlived its own binary: a
/// dev rebuild or an in-place upgrade unlinks the file we started from,
/// and `current_exe()` then reports `/path/poltertype (deleted)`, which
/// cannot be spawned. Before this, every "Settings…" click on such a
/// tray failed with `ENOENT` and did nothing visible, for ever.
fn settings_ui_exe() -> Option<PathBuf> {
    let restart = format!(
        "Restart {app} to open Settings.",
        app = crate::consts::APP_NAME
    );
    match resolve_own_exe() {
        Ok(OwnExe::Live(p)) => Some(p),
        Ok(OwnExe::Replaced(p)) => {
            // Launch the build that sits there now. It may be a
            // different version than this process, which is fine for
            // a GUI whose entire contract is "read and write
            // config.toml" — and strictly better than the alternative
            // of refusing to open at all.
            warn!(
                exe = ?p,
                "our binary was replaced on disk since startup; \
                 launching the build now at that path"
            );
            Some(p)
        }
        Ok(OwnExe::Gone(p)) => {
            warn!(exe = ?p, "our binary is gone from disk; can't open settings UI");
            spawn_error_notification(format!(
                "Couldn't open Settings: the {app} binary is no longer on \
                 disk — it was removed or replaced while the app was \
                 running.\n{restart}",
                app = crate::consts::APP_NAME,
            ));
            None
        }
        Err(e) => {
            warn!(?e, "could not locate own exe; can't open settings UI");
            spawn_error_notification(format!(
                "Couldn't open Settings: {app} can't locate its own \
                 executable ({e}).\n{restart}",
                app = crate::consts::APP_NAME,
            ));
            None
        }
    }
}
