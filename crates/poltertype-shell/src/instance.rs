//! What to hand `SingleInstance` so it means the same thing per OS.

use std::path::Path;

/// The id for the process-wide single-instance lock.
///
/// `single-instance` takes a `&str` on every platform and means
/// something different by it each time: a named mutex on Windows, an
/// abstract socket name on Linux — and on macOS a **filesystem path**
/// it `File::create`s and `flock`s. A bare identifier therefore lands
/// in the process working directory there, which is `/` when the app
/// is launched from Finder or launchd. That volume is read-only, so
/// v0.5.0 aborted at startup with "Read-only file system" and never
/// showed a tray icon.
///
/// So macOS gets an absolute path under the user's config directory,
/// and the platforms that treat the id as a name keep the name.
///
/// `config_dir` is resolved by the caller (it has the settings store
/// and its fallbacks); this only decides what to do with it. The
/// directory is created if missing — the lock is taken before the
/// settings store has had a chance to create it.
pub fn instance_lock_id(app_id: &str, config_dir: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
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
    #[cfg(not(target_os = "macos"))]
    {
        let _ = config_dir;
        app_id.to_owned()
    }
}
