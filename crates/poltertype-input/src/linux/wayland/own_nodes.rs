//! Registry of the `/dev/input/event*` nodes this process created.
//!
//! The gate must never grab our own uinput emitter — a self-grab
//! redirects our correction output back into this process and takes
//! the whole session's input with it. The exclusion used to rest on a
//! kernel-name comparison alone; the emitter now also records the
//! node path the kernel assigned at creation, so device discovery can
//! match by identity and a name drift (kernel truncation, a renamed
//! emitter, a second instance's device) can never re-open the hole.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;

static OWN_NODES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Called by the emitter right after the kernel materialises its
/// virtual keyboard. Idempotent.
pub(crate) fn record(path: PathBuf) {
    let mut nodes = OWN_NODES.lock();
    if !nodes.contains(&path) {
        nodes.push(path);
    }
}

/// Is `path` a device node this process created?
pub(crate) fn is_own(path: &Path) -> bool {
    OWN_NODES.lock().iter().any(|p| p == path)
}
