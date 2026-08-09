//! The one call the app makes: check, download, verify, stage.

use tracing::{debug, info};

use crate::download;
use crate::enums::UpdateError;
use crate::manifest;
use crate::staging;
use crate::types::PendingUpdate;
use crate::version::is_newer;

/// Version of the running binary. Single source of truth for every
/// comparison in this crate — the manifest is always measured against
/// what is *executing*, never against what is installed on disk.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Check GitHub for a newer release and, if there is one, leave a
/// verified installer in the staging directory. `None` when we are
/// already current.
///
/// Nothing is installed and no process is spawned — that is
/// [`crate::apply`], which only runs at a moment the user chose.
///
/// Safe to call repeatedly: an artifact already staged for the version
/// the manifest names is reused rather than re-downloaded.
pub fn check_and_stage() -> Result<Option<PendingUpdate>, UpdateError> {
    let current = current_version();
    let manifest = manifest::fetch()?;

    if !is_newer(&manifest.version, current)? {
        debug!(
            latest = %manifest.version,
            %current,
            "no update available"
        );
        // We are at or ahead of the published release. If an artifact is
        // still staged, it is either the update that just installed
        // successfully or one that has been superseded — either way it
        // has no future, and keeping it would leave a stale "Restart to
        // update" in the tray forever.
        if let Some(stale) = staging::read_pending() {
            info!(
                staged = %stale.version,
                %current,
                "staged update is no longer newer than the running build; clearing it"
            );
            staging::clear_pending();
        }
        return Ok(None);
    }

    // Already downloaded and verified on an earlier pass? Reuse it. The
    // artifact only exists at its final path if its checksum matched
    // (see `download::fetch_verified`), so there is nothing to re-check.
    if let Some(pending) = staging::read_pending() {
        if pending.version == manifest.version {
            debug!(version = %pending.version, "update already staged");
            return Ok(Some(pending));
        }
        info!(
            staged = %pending.version,
            available = %manifest.version,
            "a newer release superseded the staged update; re-staging"
        );
        staging::clear_pending();
    }

    let key = manifest::platform_key();
    let artifact = manifest::pick(&manifest, &key)?;

    let dir = staging::staging_dir()?;
    let path = download::fetch_verified(artifact, &dir)?;

    let pending = PendingUpdate {
        version: manifest.version.clone(),
        notes_url: manifest.notes_url.clone(),
        artifact: path,
        attempts: 0,
    };
    staging::write_pending(&pending)?;
    info!(
        version = %pending.version,
        %current,
        "update staged; it will be installed on the next restart"
    );
    Ok(Some(pending))
}
