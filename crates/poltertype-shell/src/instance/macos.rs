//! macOS `flock`s the id as a file, so a bare identifier lands in the
//! process's working directory — `/` when launched from Finder — and
//! v0.5.0 aborted with "Read-only file system". This gets an absolute
//! path under the user's config directory instead.

use std::path::Path;

pub struct InstanceLock {
    _inner: single_instance::SingleInstance,
}

pub fn acquire(app_id: &str, config_dir: &Path) -> std::io::Result<Option<InstanceLock>> {
    let inner = single_instance::SingleInstance::new(&lock_id(app_id, config_dir))
        .map_err(|e| std::io::Error::other(format!("{e}")))?;
    if inner.is_single() {
        Ok(Some(InstanceLock { _inner: inner }))
    } else {
        Ok(None)
    }
}

/// What to hand `single-instance`.
pub(crate) fn lock_id(app_id: &str, config_dir: &Path) -> String {
    if let Err(e) = std::fs::create_dir_all(config_dir) {
        // Not fatal: `File::create` below may still succeed if the
        // directory raced into existence, and if it does not, the
        // caller reports the failure to open the lock.
        tracing::warn!(
            ?e,
            ?config_dir,
            "could not create the config directory for the instance lock"
        );
    }
    config_dir
        .join(format!("{app_id}.lock"))
        .to_string_lossy()
        .into_owned()
}
