//! Linux answers for the hotkey-environment probe.

use std::time::{Duration, Instant};

pub(super) fn observed_not_consumed() -> bool {
    crate::linux::session_kind() != crate::linux::SessionKind::X11
}

/// Only Linux can come up with no hotkey backend, and it does not come
/// up gracefully: `global-hotkey`'s X11 backend opens a display on a
/// thread of its own and uses the handle without checking it, so with
/// no display its first act is `XDefaultRootWindow(NULL)` — SIGSEGV
/// inside libX11, under our name, three log lines into startup.
pub(super) fn wait_for_hotkey_backend(window: Duration) -> bool {
    let deadline = Instant::now() + window;
    loop {
        if x11rb::connect(None).is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
