//! `X11Switcher` — layout control by locking XKB groups.

use super::consts::*;
use super::xkb::*;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use tracing::debug;
use x11rb::connection::Connection;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::Window;
use x11rb::rust_connection::RustConnection;

pub struct X11Switcher {
    conn: RustConnection,
    root: Window,
}

pub fn try_init() -> Option<X11Switcher> {
    // Under a Wayland compositor the X server we can reach is XWayland,
    // and its XKB group governs X clients only — locking it would
    // "switch the layout" for half the desktop and leave Wayland-native
    // apps (and the compositor's own indicator) on the old one. The
    // compositor owns layout there, so this backend must stand down.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    // XKB is not optional in any X server built this century, but ask
    // rather than assume — a `use_extension` that reports unsupported
    // means every later request would be a protocol error.
    let xkb = conn.xkb_use_extension(1, 0).ok()?.reply().ok()?;
    if !xkb.supported {
        return None;
    }

    let root = conn.setup().roots.get(screen_num)?.root;
    // A server with no layout list configured has nothing for us to
    // switch between; fall through rather than claim the session and
    // then fail on every call.
    if read_layouts(&conn, root).ok()?.is_empty() {
        return None;
    }

    Some(X11Switcher { conn, root })
}

impl LayoutSwitcher for X11Switcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let layouts = self.list_active()?;
        let idx = locked_group(&self.conn)?;
        layouts.get(idx).cloned().ok_or_else(|| {
            LayoutError::Os(format!(
                "locked XKB group {idx} has no matching layout (list has {})",
                layouts.len()
            ))
        })
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        read_layouts(&self.conn, self.root)
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let layouts = self.list_active()?;
        let Some(idx) = layouts.iter().position(|l| l == id) else {
            return Err(LayoutError::NotActive(id.clone()));
        };
        lock_group(&self.conn, idx)?;
        debug!(layout = %id, idx, "X11 XKB group locked");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        BACKEND_NAME
    }
}
