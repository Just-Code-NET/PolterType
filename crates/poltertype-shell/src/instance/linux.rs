//! Linux does not use `single-instance` at all: its socket is not
//! close-on-exec, and this app spawns children that can outlive it — a
//! plug-in service reparented to init keeps holding the lock, and
//! PolterType then refuses to start with a message naming neither the
//! plug-in nor the possibility. Clean shutdown is not the fix — the
//! whole point is the cases where it does not happen — and closing
//! descriptors in the child means `pre_exec` and `unsafe`. So this
//! binds the abstract name with `std`, whose sockets are close-on-exec
//! by construction, keeping the exact string `single-instance` used so
//! builds from either side of the change still see each other.

use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener};
use std::path::Path;

pub struct InstanceLock {
    _socket: UnixListener,
}

pub fn acquire(app_id: &str, config_dir: &Path) -> std::io::Result<Option<InstanceLock>> {
    let _ = config_dir;
    let addr = SocketAddr::from_abstract_name(app_id.as_bytes())?;
    match UnixListener::bind_addr(&addr) {
        Ok(socket) => Ok(Some(InstanceLock { _socket: socket })),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Ok(None),
        Err(e) => Err(e),
    }
}
