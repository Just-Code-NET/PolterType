//! Spawning the Settings GUI child process and refreshing the engine
//! when it closes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

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
/// Every refresh runs on close, so any kind of edit takes effect before
/// focus returns to the user's app. The per-profile rebuild is the
/// non-obvious one: the watcher otherwise swaps dictionaries only on a
/// profile transition, so words edited while focused on a profiled app
/// would do nothing until the user alt-tabbed away and back.
///
/// A failure to spawn gets a notification, not just a log line: the
/// click produced no window, and a tray app has nowhere else to put an
/// error.
pub(crate) fn spawn_settings_ui(deps: SettingsCloseDeps) {
    spawn_settings_ui_on(deps, SettingsEntry::Normal)
}

/// Same, but opening on the Setup pane — where the tray's "keyboard
/// hooks unavailable" alert sends the user.
pub(crate) fn spawn_setup_ui(deps: SettingsCloseDeps) {
    spawn_settings_ui_on(deps, SettingsEntry::Setup)
}

/// Whether a Settings child is running right now.
///
/// The window is a subprocess and the tray had no memory of it, so
/// every click on the menu item started another one — three clicks,
/// three windows, each writing `config.toml` on Save and the last one
/// winning (issue #53).
static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

fn spawn_settings_ui_on(deps: SettingsCloseDeps, entry: SettingsEntry) {
    // Claimed before the spawn and released by the waiter below, so a
    // second click during the seconds the window takes to appear is
    // refused too — that is exactly when a user clicks again.
    if SETTINGS_OPEN.swap(true, Ordering::AcqRel) {
        info!("settings UI is already open; not opening a second window");
        return;
    }
    let Some(exe) = settings_ui_exe() else {
        SETTINGS_OPEN.store(false, Ordering::Release);
        return;
    };
    info!(?exe, ?entry, "launching settings UI");
    let child = match std::process::Command::new(&exe).arg(entry.flag()).spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(?e, ?exe, "settings UI subprocess failed to start");
            SETTINGS_OPEN.store(false, Ordering::Release);
            spawn_error_notification(format!(
                "Couldn't open Settings: {e}.\nRestarting {app} should fix it.",
                app = crate::consts::APP_NAME,
            ));
            return;
        }
    };

    SETTINGS_CHILD_PID.store(child.id(), Ordering::Release);

    // Waited on in a worker thread so the tray does not block. The
    // refreshes run whether or not the user clicked Save — the GUI
    // writes files outside its own state too.
    std::thread::Builder::new()
        .name("poltertype-settings-waiter".into())
        .spawn(move || {
            let mut child = child;
            let waited = child.wait();
            SETTINGS_CHILD_PID.store(0, Ordering::Release);
            match waited {
                Ok(status) => info!(?status, "settings UI exited"),
                Err(e) => warn!(?e, "could not wait on settings UI child"),
            }
            // Released here rather than at the end: everything below is
            // reloading files, and a user who closes the window and
            // immediately reopens it must not be told to wait for that.
            SETTINGS_OPEN.store(false, Ordering::Release);

            match deps.settings.reload() {
                Ok(changed) => info!(changed, "config.toml reloaded after settings UI exit"),
                Err(e) => warn!(?e, "could not reload config.toml after settings UI exit"),
            }

            // The autostart checkbox edits config.toml like any other
            // setting; re-apply it to the OS now that the file is
            // re-read.
            poltertype_autostart::sync(
                deps.settings.snapshot().general.autostart,
                poltertype_autostart::App {
                    id: crate::consts::APP_ID,
                    name: crate::consts::APP_NAME,
                    icon: poltertype_shell::DESKTOP_ID,
                },
            );

            // Same path as the tray's "Reload Settings" menu entry.
            let n = reload_user_dictionaries(&deps.dict_reload_handle);
            info!(
                loaded = n,
                "wordlist dictionaries reloaded after settings UI exit"
            );

            // Rebuild even when no profiles are configured — an empty
            // cache is cheap and keeps the contract uniform.
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

            // Sent last so any observer sees the rebuilds before the
            // engine command.
            if let Err(e) = deps.reload_tx.send(EngineCommand::SettingsReloaded) {
                warn!(?e, "could not enqueue SettingsReloaded after UI exit");
            }
            // The tray, separately: it owns the hotkey grabs, and a
            // chord changed in the window it just closed has to start
            // working now rather than after a restart.
            if deps
                .proxy
                .send_event(crate::enums::UserEvent::SettingsChanged)
                .is_err()
            {
                warn!("tray is gone; hotkeys not re-applied after settings UI exit");
            }
        })
        .ok();
}

/// Which binary to hand to `Command::new`, or `None` when there is
/// nothing launchable — in which case the user has already been told.
/// The interesting case is a tray that has outlived its own binary;
/// see [`OwnExe`]. Left unhandled it makes every "Settings…" click fail
/// with `ENOENT` and do nothing visible, for ever.
fn settings_ui_exe() -> Option<PathBuf> {
    let restart = format!(
        "Restart {app} to open Settings.",
        app = crate::consts::APP_NAME
    );
    match resolve_own_exe() {
        Ok(OwnExe::Live(p)) => Some(p),
        Ok(OwnExe::Replaced(p)) => {
            // A different version is fine for a GUI whose whole
            // contract is "read and write config.toml", and beats
            // refusing to open at all.
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

/// Pid of the live Settings window, 0 when none. One window at a time
/// is already the rule; this is how the main process can take it along
/// when it exits.
static SETTINGS_CHILD_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Close the Settings window before the main process exits.
///
/// The window is a subprocess running the same executable, and left
/// alive it does worse than linger: on macOS the updater's relaunch
/// `open` sees "this app is already running", reports success, and
/// merely brings the ORPHANED OLD-VERSION settings window to the
/// front — the updated app never starts and the user is looking at
/// the previous version's Settings (measured, twice). Quit has the
/// same leak without the relaunch twist.
pub(crate) fn kill_settings_ui() {
    let pid = SETTINGS_CHILD_PID.swap(0, Ordering::AcqRel);
    if pid != 0 {
        info!(pid, "closing the settings window before exit");
    }
    #[cfg(unix)]
    {
        if pid != 0 {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }
        // And every other window of this executable, tracked or not:
        // this base has no second-window guard, so a Settings window
        // and a Setup-alert window can coexist while one pid slot
        // remembers only the latest. The one that survived the main
        // process is what made LaunchServices treat the app as still
        // running and turned the updater relaunch into a no-op that
        // merely raised an orphaned old-version window (measured:
        // `poltertype --setup`, 2026-08-31).
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new("pkill")
                .arg("-f")
                .arg(format!("{} --", exe.display()))
                .status();
        }
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status();
    }
}
