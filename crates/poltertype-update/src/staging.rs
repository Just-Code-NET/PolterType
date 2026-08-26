//! The staging directory and its `pending.json` bookkeeping.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use directories::ProjectDirs;
use tracing::{debug, info, warn};

use crate::consts::*;
use crate::enums::UpdateError;
use crate::types::PendingUpdate;

/// The app's data directory. Same ProjectDirs triple the rest of the
/// app uses, so a user who wipes it wipes the staged update with it.
fn data_dir() -> Result<PathBuf, UpdateError> {
    let dirs =
        ProjectDirs::from("dev", "opensource", "poltertype").ok_or(UpdateError::NoDataDir)?;
    Ok(dirs.data_local_dir().to_path_buf())
}

/// `<data_local_dir>/poltertype/updates/`.
pub(crate) fn staging_dir() -> Result<PathBuf, UpdateError> {
    Ok(data_dir()?.join(STAGING_DIR))
}

/// Where the installer script's own output goes — see
/// [`crate::consts::INSTALLER_LOG`] for why it is not in the staging
/// directory.
pub(crate) fn installer_log_path() -> Result<PathBuf, UpdateError> {
    Ok(data_dir()?.join(LOG_DIR).join(INSTALLER_LOG))
}

pub(crate) fn pending_path() -> Result<PathBuf, UpdateError> {
    Ok(staging_dir()?.join(PENDING_FILE))
}

/// The staged update, if there is one *and* its artifact still exists.
///
/// A `pending.json` whose artifact has been deleted counts as no
/// pending update, and the stale record is removed — the alternative is
/// a "Restart to update" item that does nothing.
///
/// `None` rather than an error for anything malformed: a corrupt
/// bookkeeping file must not be able to break app startup.
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

/// The exit code a refused install left behind, read once and cleared.
///
/// The marker is written by the installer script and by nothing else,
/// so its presence means the script ran and the OS turned the artifact
/// down — which is the one thing the app could never tell apart from an
/// installer that never started at all.
pub fn take_install_failure() -> Option<String> {
    let path = staging_dir().ok()?.join(FAILED_FILE);
    let text = fs::read_to_string(&path).ok()?;
    if let Err(e) = fs::remove_file(&path) {
        warn!(?e, ?path, "could not clear the install-failure marker");
    }
    let reason = text.trim().to_owned();
    (!reason.is_empty()).then_some(reason)
}

/// Whether this artifact has any tries left — and if not, get rid of it.
///
/// Without the ceiling, a file the OS installer rejects every single
/// time would be retried on every quit, forever.
pub(crate) fn attempts_left(pending: &PendingUpdate) -> bool {
    if pending.attempts >= MAX_INSTALL_ATTEMPTS {
        warn!(
            version = %pending.version,
            attempts = pending.attempts,
            "staged update has failed to install too many times; discarding it"
        );
        clear_pending();
        return false;
    }
    true
}

/// Record that an installer is running with this artifact.
///
/// Counted once the installer has confirmed it is alive, not merely
/// once it has been spawned. The difference is the whole point: an
/// installer the OS never actually ran is not evidence against the
/// artifact, and counting it burned three good downloads for a user
/// whose PowerShell could not start at all.
pub(crate) fn note_install_attempt(pending: &PendingUpdate) {
    let bumped = PendingUpdate {
        attempts: pending.attempts + 1,
        ..pending.clone()
    };
    if let Err(e) = write_pending(&bumped) {
        // Not fatal: we lose the retry accounting for this one attempt,
        // but blocking a legitimate install because we couldn't write a
        // counter would be the worse trade.
        warn!(?e, "could not record the install attempt");
    }
}
