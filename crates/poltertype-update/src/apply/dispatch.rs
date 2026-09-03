//! Picks the OS backend once, by `cfg`, and hands it the staged
//! update.

use tracing::info;

use crate::enums::{Applied, UpdateError};
use crate::staging;
use crate::types::PendingUpdate;

#[cfg(target_os = "linux")]
use super::linux as imp;
#[cfg(target_os = "macos")]
use super::macos as imp;
#[cfg(target_os = "windows")]
use super::windows as imp;

/// Install the staged update, then leave.
///
/// **On [`Applied::HandedOff`] the caller must exit promptly** — an
/// installer may be blocked on our process disappearing. `relaunch`
/// says whether PolterType should be started again afterwards; the
/// other two outcomes both mean *keep running*, and are the difference
/// between an update that is not coming and one that has arrived but
/// cannot restart us.
///
/// `macos_sign_identity` is `[updates].local_signing_identity`: on
/// macOS the installer re-signs the swapped bundle with it (TCC grants
/// then survive the update), or, when empty, resets the two stale TCC
/// records so the Setup pane can re-ask cleanly. The other platforms
/// ignore it.
pub fn apply(
    pending: &PendingUpdate,
    relaunch: bool,
    macos_sign_identity: &str,
) -> Result<Applied, UpdateError> {
    if !staging::attempts_left(pending) {
        return Ok(Applied::Discarded);
    }

    info!(
        version = %pending.version,
        artifact = ?pending.artifact,
        relaunch,
        "handing the staged update to the OS installer"
    );

    imp::apply(pending, relaunch, macos_sign_identity)
}
