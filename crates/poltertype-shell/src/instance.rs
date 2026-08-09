//! The process-wide single-instance lock.
//!
//! **The id means something different on every platform.**
//! `single-instance` takes a `&str` everywhere but reads it as a named
//! mutex on Windows, an abstract socket name on Linux, and on macOS a
//! **filesystem path** it creates and `flock`s. A bare identifier
//! therefore landed in the working directory — `/` when launched from
//! Finder — and v0.5.0 aborted with "Read-only file system". macOS gets
//! an absolute path under the user's config directory; the platforms
//! that treat the id as a name keep the name.
//!
//! **Linux does not use `single-instance` at all**, because its socket
//! is not close-on-exec and this app spawns children that can outlive
//! it: a plug-in service reparented to init keeps holding the lock, and
//! PolterType then refuses to start with a message naming neither the
//! plug-in nor the possibility. Clean shutdown is not the fix — the
//! whole point is the cases where it does not happen — and closing
//! descriptors in the child means `pre_exec` and `unsafe` in a crate
//! that deliberately has none. So Linux binds the abstract name with
//! `std`, whose sockets are close-on-exec by construction. The name is
//! deliberately the same string `single-instance` used, so builds from
//! either side of the change still see each other.

use std::path::Path;

/// A held single-instance lock.
///
/// Dropping it releases the lock, and so does the process ending for
/// any reason at all, including one that runs no destructors — what is
/// held is a kernel object. That is the property being bought: no stale
/// lock to clean up after a crash, and no "delete this file to recover"
/// instruction for anyone to find.
pub struct InstanceLock {
    #[cfg(target_os = "linux")]
    _socket: std::os::unix::net::UnixListener,
    #[cfg(not(target_os = "linux"))]
    _inner: single_instance::SingleInstance,
}

/// Try to become the only running instance.
///
/// `Ok(None)` means somebody else already is — the ordinary case, not
/// an error. `Err` means the question could not be answered at all,
/// which is worth reporting rather than guessing either way.
pub fn acquire(app_id: &str, config_dir: &Path) -> std::io::Result<Option<InstanceLock>> {
    #[cfg(target_os = "linux")]
    {
        let _ = config_dir;
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixListener};

        let addr = SocketAddr::from_abstract_name(app_id.as_bytes())?;
        match UnixListener::bind_addr(&addr) {
            Ok(socket) => Ok(Some(InstanceLock { _socket: socket })),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Ok(None),
            Err(e) => Err(e),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let inner = single_instance::SingleInstance::new(&lock_id(app_id, config_dir))
            .map_err(|e| std::io::Error::other(format!("{e}")))?;
        if inner.is_single() {
            Ok(Some(InstanceLock { _inner: inner }))
        } else {
            Ok(None)
        }
    }
}

/// What to hand `single-instance` on the platforms that still use it.
#[cfg(not(target_os = "linux"))]
pub(crate) fn lock_id(app_id: &str, config_dir: &Path) -> String {
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
