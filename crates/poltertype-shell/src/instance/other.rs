//! Windows and anything else: the plain `single-instance` named mutex,
//! with no path to build.

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

fn lock_id(app_id: &str, config_dir: &Path) -> String {
    let _ = config_dir;
    app_id.to_owned()
}
