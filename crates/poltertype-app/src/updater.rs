//! The background update worker and the tray's side of updating.
//!
//! Policy here, mechanism in `poltertype-update`. The rule this module
//! exists to enforce: **an update is never installed while the app is
//! running** — swapping the binary out from under a live keyboard hook
//! is the one thing an app like this must never do. Checking and
//! downloading happen on a worker thread; installing happens only at a
//! moment the user picked.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::settings::SettingsStore;
use poltertype_update::{Applied, PendingUpdate, UpdateError};
use tao::event_loop::EventLoopProxy;
use tracing::{info, warn};
use tray_icon::menu::MenuItem;

use crate::bridges::spawn_error_notification;
use crate::consts::*;
use crate::enums::*;

/// How long after startup the first check waits.
///
/// Launch is the busiest moment this app has, and a login-time autostart
/// has every other app competing for the same disk and network. A minute
/// of quiet costs nothing — the update has been out for hours.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);

/// How often the worker wakes to ask whether a check is due.
///
/// The interval itself is measured against the wall clock rather than
/// slept through: `recv_timeout` counts monotonic time, which does not
/// advance while a machine is suspended. A laptop that is closed every
/// night never accumulates twenty-four hours of it, so the check that
/// runs "daily" runs on the day the user reboots and never again —
/// which is exactly what an Apple Silicon tester reported in #3.
/// Waking a few times an hour to compare two timestamps costs nothing
/// and survives suspend.
const POLL: Duration = Duration::from_secs(15 * 60);

/// The staged update this build should actually offer, if any.
///
/// A `pending.json` that is no longer newer than us is the fingerprint of
/// a *successful* install: we staged 0.4.0, the user quit, the installer
/// ran, and this 0.4.0 process is looking at the record of its own
/// arrival. Clearing it is what stops the tray offering "Restart to
/// update — v0.4.0" to a user already running 0.4.0.
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

pub(crate) fn menu_label(pending: Option<&PendingUpdate>) -> String {
    match pending {
        Some(p) => tr_args(
            "tray.restart_to_update",
            "⟳ Restart to update — v{}",
            &[&p.version],
        ),
        None => tr("tray.check_updates", "Check for updates…").to_owned(),
    }
}

pub(crate) fn refresh_menu_item(item: &MenuItem, pending: Option<&PendingUpdate>) {
    item.set_text(menu_label(pending));
}

/// Report an install the OS refused last time, once.
///
/// The marker the installer leaves behind is the only trace a refused
/// install has: the app it was meant to replace is still the one
/// running, and from the user's side a "Restart to update" that fails
/// is indistinguishable from one that did nothing at all. Consumed on
/// read, so this is said once rather than at every start.
///
/// Must run before [`pending_for_this_build`], which may clear the
/// staging directory the marker lives in.
pub(crate) fn report_previous_install_failure() {
    let Some(reason) = poltertype_update::take_install_failure() else {
        return;
    };
    warn!(%reason, "the previous update install was refused");
    spawn_error_notification(format!(
        "PolterType could not install its last update.\n{reason}\n\
         The installer's own log is in the logs folder \
         (Tray → \"Open Logs Folder…\").\n\
         Download it from {RELEASES_URL}"
    ));
}

/// Install the staged update and say whether the app must now exit.
///
/// `relaunch` distinguishes the two ways here: the user clicked
/// "Restart to update" and expects the app back, or clicked Quit and
/// expects it gone.
///
/// `false` means the app must **stay running** — because nothing is
/// coming, or because the new build is in place but nothing on this
/// session can start us again. An app that quits for a restart that
/// never happens is an app the user has to go and start by hand, which
/// is precisely how a failing updater turned into a machine with no
/// PolterType on it.
pub(crate) fn apply_now(pending: &PendingUpdate, relaunch: bool, sign_identity: &str) -> bool {
    match poltertype_update::apply(pending, relaunch, sign_identity) {
        Ok(Applied::HandedOff) => {
            info!(
                version = %pending.version,
                relaunch,
                "update handed off; exiting so it can take over"
            );
            return true;
        }
        Ok(Applied::InstalledStayUp) => {
            // The one outcome that is neither success nor failure: the
            // user gets the new version, just not this second.
            info!(
                version = %pending.version,
                "update installed, but nothing here can restart us; staying up"
            );
            spawn_error_notification(format!(
                "PolterType {} is installed, but this session could not start it again.\n\
                 It will be the version you get the next time PolterType starts.",
                pending.version
            ));
        }
        Ok(Applied::Discarded) => {
            // Three refused installs in a row: the artifact is gone and
            // the tray is about to go back to "Check for updates…".
            // Silence here would read as the button doing nothing.
            info!("staged update was discarded after repeated install failures");
            spawn_error_notification(format!(
                "PolterType {} could not be installed after several attempts.\n\
                 Download it from {RELEASES_URL}",
                pending.version
            ));
        }
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
    false
}

/// Run the periodic check on its own thread.
///
/// `ureq` is blocking and a download can take minutes on a bad link, so
/// the event loop only ever receives a finished result through the proxy.
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
            let mut due = SystemTime::now() + FIRST_CHECK_DELAY;
            loop {
                // Sleeping on the channel rather than `thread::sleep` is
                // what makes a manual check instant.
                let wait = due
                    .duration_since(SystemTime::now())
                    .unwrap_or(Duration::ZERO)
                    .min(POLL);
                match check_now.recv_timeout(wait) {
                    Ok(()) => info!("manual update check requested"),
                    // A tick, not necessarily a due check: the poll is
                    // deliberately shorter than the interval.
                    Err(RecvTimeoutError::Timeout) if SystemTime::now() < due => continue,
                    Err(RecvTimeoutError::Timeout) => {}
                    // The tray dropped its sender: the app is shutting
                    // down and there is nobody left to report to.
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                let cfg = settings.snapshot().updates;
                due = SystemTime::now() + cfg.interval();

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
                        // proxies exist. Logged, never shown.
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
