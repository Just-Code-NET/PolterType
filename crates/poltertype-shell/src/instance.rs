//! The process-wide single-instance lock.
//!
//! Two separate problems live here, and both were found the hard way.
//!
//! ## What the id means
//!
//! `single-instance` takes a `&str` on every platform and means
//! something different by it each time: a named mutex on Windows, an
//! abstract socket name on Linux — and on macOS a **filesystem path**
//! it `File::create`s and `flock`s. A bare identifier therefore lands
//! in the process working directory there, which is `/` when the app
//! is launched from Finder or launchd. That volume is read-only, so
//! v0.5.0 aborted at startup with "Read-only file system" and never
//! showed a tray icon.
//!
//! So macOS gets an absolute path under the user's config directory,
//! and the platforms that treat the id as a name keep the name.
//!
//! ## Why Linux does not use `single-instance` at all
//!
//! Because its socket is not close-on-exec, and this app spawns
//! children that can outlive it.
//!
//! `single-instance` 0.3 creates the abstract socket with
//! `SockFlag::empty()`, so the descriptor survives `exec()` into every
//! process we start. A plug-in service is exactly such a process: it is
//! spawned by the tray, and if the tray is killed rather than asked to
//! quit — `kill -9`, a crash, an OOM — the plug-in is reparented to
//! init and **keeps holding PolterType's lock**. PolterType then
//! refuses to start for as long as that plug-in lives, and says only
//! "another instance is already running": a message naming neither the
//! plug-in nor the possibility that a plug-in is the cause. There is
//! nothing in it the user can act on.
//!
//! Shutting down cleanly is not the fix — the whole point is the cases
//! where that does not happen. Nor is closing descriptors in the child,
//! which means `pre_exec` and `unsafe`, in a crate that deliberately
//! has none.
//!
//! The fix is not to leak it. `std`'s sockets are close-on-exec by
//! construction, and binding an abstract name has been safe stable Rust
//! since 1.70 — so on Linux the lock is a dozen lines of `std` and the
//! descriptor cannot reach a child at all. Windows (a named mutex, not
//! inherited unless explicitly requested) and macOS (a `File`, which
//! `std` also opens close-on-exec) never had the problem and keep the
//! dependency.
//!
//! The abstract name is deliberately the same string `single-instance`
//! used, so a build from before this change and one from after still
//! see each other rather than both starting.

use std::path::Path;

/// A held single-instance lock.
///
/// Dropping it releases the lock — and so does the process ending for
/// any reason at all, including one that runs no destructors, because
/// what is held is a kernel object and the kernel closes it. That is
/// the property being bought here: there is no stale lock to clean up
/// after a crash, and therefore no "delete this file to recover"
/// instruction anyone has to find.
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
///
/// `config_dir` is resolved by the caller (it has the settings store
/// and its fallbacks); this only decides what to do with it.
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
