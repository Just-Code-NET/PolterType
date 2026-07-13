//! The staging directory and its `pending.json` bookkeeping.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use directories::ProjectDirs;
use tracing::{debug, info, warn};

use crate::consts::*;
use crate::enums::UpdateError;
use crate::types::PendingUpdate;

/// `<data_local_dir>/poltertype/updates/`. Same ProjectDirs triple the
/// rest of the app uses, so a user who wipes the app's data directory
/// wipes the staged update with it.
pub(crate) fn staging_dir() -> Result<PathBuf, UpdateError> {
    let dirs =
        ProjectDirs::from("dev", "opensource", "poltertype").ok_or(UpdateError::NoDataDir)?;
    Ok(dirs.data_local_dir().join(STAGING_DIR))
}

pub(crate) fn pending_path() -> Result<PathBuf, UpdateError> {
    Ok(staging_dir()?.join(PENDING_FILE))
}

/// The staged update, if there is one *and* its artifact still exists.
///
/// A `pending.json` whose artifact has been deleted (user cleaned their
/// cache, a disk tool swept the staging dir) is treated as no pending
/// update at all, and the stale record is removed. The alternative —
/// reporting a pending update we can't install — would show the user a
/// "Restart to update" item that does nothing.
///
/// Returns `None` rather than an error for anything malformed: a
/// corrupt bookkeeping file must not be able to break app startup.
pub fn read_pending() -> Option<PendingUpdate> {
    let path = pending_path().ok()?;
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            warn!(?e, ?path, "could not read pending-update record");
            return None;
        }
    };
    let pending: PendingUpdate = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => {
            warn!(?e, ?path, "pending-update record is malformed; discarding");
            clear_pending();
            return None;
        }
    };
    if !pending.artifact.is_file() {
        warn!(
            artifact = ?pending.artifact,
            "staged artifact is gone; discarding the pending-update record"
        );
        clear_pending();
        return None;
    }
    Some(pending)
}

pub(crate) fn write_pending(pending: &PendingUpdate) -> Result<(), UpdateError> {
    let path = pending_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(pending)?;
    fs::write(&path, json)?;
    debug!(?path, version = %pending.version, "wrote pending-update record");
    Ok(())
}

/// Forget the staged update: delete the artifact and the record.
///
/// Called when the update installs successfully (the running version
/// has caught up), when the artifact turns out to be un-installable
/// after [`MAX_INSTALL_ATTEMPTS`], and when the user turns updates off.
/// Best-effort — a staging directory we failed to clean is a few tens
/// of MB, not a broken app, so nothing here propagates an error.
pub fn clear_pending() {
    let Ok(dir) = staging_dir() else {
        return;
    };
    if !dir.exists() {
        return;
    }
    match fs::remove_dir_all(&dir) {
        Ok(()) => info!(?dir, "cleared staged update"),
        Err(e) => warn!(?e, ?dir, "could not clear the staging directory"),
    }
}

/// Record that we are about to hand the staged artifact to the OS
/// installer, and say whether it is still worth trying.
///
/// The counter is bumped *before* the attempt, not after: an installer
/// that hard-kills our process (or a machine that loses power mid
/// install) would never reach an after-the-fact increment, and the
/// broken artifact would be retried on every single quit. Counting the
/// attempt up front means each try costs one, whatever happens next.
pub(crate) fn note_install_attempt(pending: &PendingUpdate) -> bool {
    let attempts = pending.attempts + 1;
    if attempts > MAX_INSTALL_ATTEMPTS {
        warn!(
            version = %pending.version,
            attempts = pending.attempts,
            "staged update has failed to install too many times; discarding it"
        );
        clear_pending();
        return false;
    }
    let bumped = PendingUpdate {
        attempts,
        ..pending.clone()
    };
    if let Err(e) = write_pending(&bumped) {
        // Not fatal: we lose the retry accounting for this one attempt,
        // but blocking a legitimate install because we couldn't write a
        // counter would be the worse trade.
        warn!(?e, "could not record the install attempt");
    }
    true
}
