//! Settings GUI child-process management.

use std::sync::atomic::Ordering;

use poltertype_core::engine::EngineCommand;
use tracing::{info, warn};

use crate::detectors::*;
use crate::types::*;

/// Spawn the Settings GUI as a child process (`poltertype
/// --settings`) and refresh the engine when the window closes.
/// Subprocess instead of in-process for the macOS main-thread
/// reason documented at the top of `settings_ui.rs`.
///
/// What "refresh" means in practice — we run all three on close so
/// every kind of edit the user could have made via the GUI takes
/// effect by the time the focus returns to their app:
///
/// 1. **`config.toml` reload** — picks up `[[commands]]` text-trigger
///    entries, hotkey rebindings, exception list edits, profile
///    schema changes.
/// 2. **Global wordlist reload** — `<config-dir>/wordlists/<stem>.txt`
///    re-read; the engine's dictionary set swapped via
///    `DictionaryDetector::replace_dicts` (same primitive the tray
///    "Reload Settings" entry uses).
/// 3. **Per-profile wordlist cache rebuild + force-reapply** — the
///    profile dictionary cache is rebuilt from disk, and we set the
///    watcher's `force_reapply` flag so the *currently active*
///    profile's freshly-loaded dicts get re-applied on the next tick
///    (~250 ms). Without this, the watcher only swaps on profile
///    transitions, so a user editing words while focused on a
///    profiled app would see no effect until they alt-tabbed away
///    and back.
///
/// Best-effort: if we can't even locate our own exe (highly unusual,
/// e.g. running from a deleted binary) we log + skip rather than
/// taking down the tray.
pub(crate) fn spawn_settings_ui(deps: SettingsCloseDeps) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            warn!(?e, "could not locate own exe; can't open settings UI");
            return;
        }
    };
    info!(?exe, "launching settings UI");
    let child = match std::process::Command::new(&exe).arg("--settings").spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(?e, ?exe, "settings UI subprocess failed to start");
            return;
        }
    };

    // Wait for the child in a worker thread so the tray doesn't
    // block. On exit we run the three refresh steps documented on
    // the function. We do all three regardless of whether the user
    // clicked Save — the GUI also has an "Open config.toml" button
    // and a Wordlists pane Save that writes files outside the
    // GUI's own state, so reload-on-close gives the most
    // predictable contract: "everything you did in the GUI applies
    // now."
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
