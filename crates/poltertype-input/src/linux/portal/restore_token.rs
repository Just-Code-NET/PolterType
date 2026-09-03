//! The on-disk restore token that lets a granted session skip the
//! consent dialog on the next launch.

use std::path::PathBuf;

use tracing::warn;

use super::consts::RESTORE_TOKEN_FILE;

fn restore_token_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("dev", "opensource", "poltertype")
        .map(|d| d.data_local_dir().join(RESTORE_TOKEN_FILE))
}

pub(super) fn load_restore_token() -> Option<String> {
    let path = restore_token_path()?;
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

/// Store the token so the next launch is silent.
///
/// Best-effort: a machine where this cannot be written just prompts
/// again next time, which is annoying rather than broken.
pub(super) fn store_restore_token(token: &str) {
    let Some(path) = restore_token_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, token) {
        warn!(path = %path.display(), %e, "could not store the portal restore token");
    }
}
