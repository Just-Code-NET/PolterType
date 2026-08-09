//! The background update worker and the tray's side of updating.
//!
//! Policy here, mechanism in `poltertype-update`. The rule this module
//! exists to enforce: **an update is never installed while the app is
//! running.** Checking and downloading happen on a worker thread;
//! installing happens only at a moment the user picked. Swapping the
//! binary out from under a live keyboard hook is the one thing an app
//! like this must never do.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use poltertype_core::settings::SettingsStore;
use poltertype_update::{PendingUpdate, UpdateError};
use tao::event_loop::EventLoopProxy;
use tracing::{info, warn};
use tray_icon::menu::MenuItem;

use crate::bridges::spawn_error_notification;
use crate::consts::*;
use crate::enums::*;

/// How long after startup the first check waits.
///
/// Launch is the busiest moment this app has — layout DB, FSTs, hooks,
/// tray — and it is also when a login-time autostart has every other
/// app on the machine competing for the same disk and network. A minute
/// of quiet costs nothing (the update has been out for hours) and keeps
/// the updater off the critical path of "did my keyboard hook come up".
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);

/// The staged update this build should actually offer, if any.
///
/// Called at startup, before the tray is built. A `pending.json` that is
/// no longer newer than us is the fingerprint of a *successful* install:
/// we staged 0.4.0, the user quit, the installer ran, and this 0.4.0
/// process is now looking at the record of its own arrival. Clearing it
/// is what stops the tray showing "Restart to update — v0.4.0" to a user
/// who is already running 0.4.0.
pub(crate) fn pending_for_this_build() -> Option<PendingUpdate> {
    let pending = poltertype_update::read_pending()?;
    let current = poltertype_update::current_version();
    match poltertype_update::is_newer(&pending.version, current) {
        Ok(true) => {
            info!(staged = %pending.version, %current, "a staged update is waiting to install");
            Some(pending)
        }
        Ok(false) => {
            info!(
                staged = %pending.version,
                %current,
                "the staged update is already installed; clearing the staging directory"
            );
            poltertype_update::clear_pending();
            None
        }
        Err(e) => {
            warn!(?e, "staged update has an unreadable version; discarding it");
            poltertype_update::clear_pending();
            None
        }
    }
}

/// Text for the tray's update entry, given what we currently know.
pub(crate) fn menu_label(pending: Option<&PendingUpdate>) -> String {
    match pending {
        Some(p) => format!("⟳ Restart to update — v{}", p.version),
        None => "Check for updates…".to_owned(),
    }
}

/// Refresh the tray entry after the worker reports in.
pub(crate) fn refresh_menu_item(item: &MenuItem, pending: Option<&PendingUpdate>) {
    item.set_text(menu_label(pending));
}

/// Install the staged update and tell the caller whether to exit.
///
/// `relaunch` distinguishes the two ways here: the user clicked
/// "Restart to update" and expects the app back, or clicked Quit and
/// expects it gone. Either way the install happens *after* we exit.
///
/// A failure is reported and swallowed — refusing to quit because an
/// installer could not be spawned would hold the user's app hostage to
/// our update mechanism. The staged artifact stays for the next try.
pub(crate) fn apply_now(pending: &PendingUpdate, relaunch: bool) {
    match poltertype_update::apply(pending, relaunch) {
        Ok(true) => info!(
            version = %pending.version,
            relaunch,
            "installer spawned; exiting so it can replace us"
        ),
        Ok(false) => info!("staged update was discarded after repeated install failures"),
        Err(UpdateError::UnsupportedInstall(reason)) => {
            // Not a bug and not worth a scary error: a dev build, a
            // distro package, a bare binary. We simply are not the
            // owner of this install and must not overwrite it.
            warn!(%reason, "this install cannot update itself; leaving it alone");
            spawn_error_notification(format!(
                "PolterType {} is ready, but this install can't update itself.\n\
                 {reason}\n\
                 Download it from {RELEASES_URL}",
                pending.version
            ));
        }
        Err(e) => {
            warn!(?e, "could not start the update installer");
            spawn_error_notification(format!(
                "Could not install PolterType {}.\n{e}\n\
                 Download it from {RELEASES_URL}",
                pending.version
            ));
        }
    }
}

/// Run the periodic check on its own thread.
///
/// Arranged so the network happens *to* a background thread and never
/// to the tray: `ureq` is blocking, a download can take minutes on a
/// bad link, and the event loop only ever receives a finished result
/// through the proxy.
///
/// `check_now` lets "Check for updates…" interrupt the sleep, so a user
/// who wants to know now does not wait out a 24-hour timer.
pub(crate) fn spawn_update_worker(
    proxy: EventLoopProxy<UserEvent>,
    settings: Arc<SettingsStore>,
    check_now: Receiver<()>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("poltertype-updater".into())
        .spawn(move || {
            let mut delay = FIRST_CHECK_DELAY;
            loop {
                // Sleeping on the channel rather than `thread::sleep` is
                // what makes a manual check instant instead of "instant,
                // in up to 24 hours".
                match check_now.recv_timeout(delay) {
                    Ok(()) => info!("manual update check requested"),
                    Err(RecvTimeoutError::Timeout) => {}
                    // The tray dropped its sender: the app is shutting
                    // down and there is nobody left to report to.
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                let cfg = settings.snapshot().updates;
                delay = cfg.interval();

                if !cfg.enabled {
                    // The user turned updates off — possibly while an
                    // artifact was already staged. Honouring the setting
                    // means getting rid of it, not just declining to
                    // fetch the next one.
                    if poltertype_update::read_pending().is_some() {
                        info!("updates disabled; discarding the staged artifact");
                        poltertype_update::clear_pending();
                        if proxy
                            .send_event(UserEvent::Update(UpdateOutcome::Cleared))
                            .is_err()
                        {
                            break;
                        }
                    }
                    continue;
                }

                let outcome = match poltertype_update::check_and_stage() {
                    Ok(Some(pending)) => UpdateOutcome::Staged(Box::new(pending)),
                    Ok(None) => UpdateOutcome::UpToDate,
                    Err(e) => {
                        // A failed check is routine — laptops sleep,
                        // planes have no wifi, corporate proxies exist.
                        // It is logged, never shown: an app that pops a
                        // dialog every time it can't reach GitHub is an
                        // app people uninstall.
                        warn!(?e, "update check failed");
                        UpdateOutcome::Failed
                    }
                };

                if proxy.send_event(UserEvent::Update(outcome)).is_err() {
                    break;
                }
            }
            info!("update worker stopped");
        })
        .context("spawn update worker thread")?;
    Ok(())
}
