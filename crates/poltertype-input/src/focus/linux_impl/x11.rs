//! X11 focus tracker (`_NET_ACTIVE_WINDOW`).

use std::sync::Arc;

use parking_lot::Mutex;
use tracing::debug;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use crate::focus::{CaretHint, FocusTracker, FocusedWindowGeometry};

use super::atspi_caret::{AtspiCaretWatcher, CaretSample};
use super::proc_exe::exe_basename_for_pid;

/// A live display connection plus the two EWMH atoms we query. Built
/// lazily on the first `focused_exe()` call and dropped whenever the
/// connection dies (the next call reconnects).
struct X11FocusConn {
    conn: RustConnection,
    root: Window,
    net_active_window: Atom,
    net_wm_pid: Atom,
}

/// Focus via EWMH: `_NET_ACTIVE_WINDOW` on the root window names the
/// focused window, `_NET_WM_PID` on that window names its process,
/// `/proc` turns the PID into an executable basename. `WM_CLASS` is
/// the fallback when `/proc` is unreadable or the WM didn't set a PID.
pub(crate) struct X11FocusTracker {
    state: Mutex<Option<X11FocusConn>>,
    /// Shared AT-SPI caret watcher; `None` when the a11y bus is
    /// unavailable (the tooltip then anchors to the window).
    caret: Option<Arc<AtspiCaretWatcher>>,
}

impl X11FocusTracker {
    pub(crate) fn new(caret: Option<Arc<AtspiCaretWatcher>>) -> Self {
        Self {
            state: Mutex::new(None),
            caret,
        }
    }
}

impl FocusTracker for X11FocusTracker {
    fn focused_exe(&self) -> Option<String> {
        let mut state = self.state.lock();
        if state.is_none() {
            match connect() {
                Ok(c) => *state = Some(c),
                Err(e) => {
                    debug!(%e, "x11 focus: connect failed");
                    return None;
                }
            }
        }
        let s = state.as_ref()?;
        match query_focused_exe(s) {
            Ok(v) => v,
            // The connection is dead (server gone, socket closed).
            // Drop it so the next call — post-cache, ≥150 ms away —
            // starts from a clean reconnect.
            Err(()) => {
                *state = None;
                None
            }
        }
    }

    fn focused_window_geometry(&self) -> Option<FocusedWindowGeometry> {
        let mut state = self.state.lock();
        if state.is_none() {
            match connect() {
                Ok(c) => *state = Some(c),
                Err(e) => {
                    debug!(%e, "x11 focus: connect failed");
                    return None;
                }
            }
        }
        let s = state.as_ref()?;
        match query_focused_geometry(s) {
            Ok(v) => v,
            Err(()) => {
                *state = None;
                None
            }
        }
    }

    fn caret_hint(&self) -> Option<CaretHint> {
        self.caret.as_ref()?.latest().map(CaretSample::into_hint)
    }

    fn backend_name(&self) -> &'static str {
        "linux-x11-ewmh"
    }
}

fn connect() -> Result<X11FocusConn, String> {
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| format!("x11 connect (is DISPLAY set?): {e}"))?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or_else(|| format!("no screen {screen_num}"))?
        .root;
    let net_active_window = intern(&conn, b"_NET_ACTIVE_WINDOW")?;
    let net_wm_pid = intern(&conn, b"_NET_WM_PID")?;
    Ok(X11FocusConn {
        conn,
        root,
        net_active_window,
        net_wm_pid,
    })
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom, String> {
    let pretty = String::from_utf8_lossy(name).into_owned();
    conn.intern_atom(false, name)
        .map_err(|e| format!("intern {pretty}: {e}"))?
        .reply()
        .map(|r| r.atom)
        .map_err(|e| format!("intern {pretty} reply: {e}"))
}

/// `Err(())` means the *connection* failed and must be rebuilt.
/// Window-level races — the active window closing between the two
/// queries (`BadWindow`) — are a normal `Ok(None)`.
fn query_focused_exe(s: &X11FocusConn) -> Result<Option<String>, ()> {
    let window = match first_u32_prop(s, s.root, s.net_active_window, AtomEnum::WINDOW.into())? {
        Some(w) if w != 0 => w,
        _ => return Ok(None),
    };
    if let Some(pid) = first_u32_prop(s, window, s.net_wm_pid, AtomEnum::CARDINAL.into())? {
        if let Some(name) = exe_basename_for_pid(pid) {
            return Ok(Some(name));
        }
    }
    wm_class_instance(s, window)
}

/// First 32-bit item of a window property, `Ok(None)` when the
/// property is unset / has the wrong format / the window is gone.
fn first_u32_prop(
    s: &X11FocusConn,
    window: Window,
    prop: Atom,
    ty: Atom,
) -> Result<Option<u32>, ()> {
    let cookie = s
        .conn
        .get_property(false, window, prop, ty, 0, 1)
        .map_err(|e| debug!(?e, "x11 focus: get_property send failed"))?;
    match cookie.reply() {
        Ok(reply) => Ok(reply.value32().and_then(|mut it| it.next())),
        Err(ReplyError::X11Error(_)) => Ok(None),
        Err(e) => {
            debug!(?e, "x11 focus: connection error");
            Err(())
        }
    }
}

/// Root-relative geometry of the active window: `GetGeometry` gives
/// the size, `TranslateCoordinates` against the root turns the
/// window-local origin into global screen coordinates (the window's
/// own x/y are relative to its WM frame parent, not the root). Same
/// error contract as the property queries: `Err(())` = dead
/// connection, window-level races = `Ok(None)`.
fn query_focused_geometry(s: &X11FocusConn) -> Result<Option<FocusedWindowGeometry>, ()> {
    let window = match first_u32_prop(s, s.root, s.net_active_window, AtomEnum::WINDOW.into())? {
        Some(w) if w != 0 => w,
        _ => return Ok(None),
    };
    let geom_cookie = s
        .conn
        .get_geometry(window)
        .map_err(|e| debug!(?e, "x11 focus: get_geometry send failed"))?;
    let geom = match geom_cookie.reply() {
        Ok(g) => g,
        Err(ReplyError::X11Error(_)) => return Ok(None),
        Err(e) => {
            debug!(?e, "x11 focus: connection error");
            return Err(());
        }
    };
    let xlate_cookie = s
        .conn
        .translate_coordinates(window, s.root, 0, 0)
        .map_err(|e| debug!(?e, "x11 focus: translate_coordinates send failed"))?;
    let xlate = match xlate_cookie.reply() {
        Ok(t) => t,
        Err(ReplyError::X11Error(_)) => return Ok(None),
        Err(e) => {
            debug!(?e, "x11 focus: connection error");
            return Err(());
        }
    };
    Ok(Some(FocusedWindowGeometry {
        x: i32::from(xlate.dst_x),
        y: i32::from(xlate.dst_y),
        width: u32::from(geom.width),
        height: u32::from(geom.height),
        pid: first_u32_prop(s, window, s.net_wm_pid, AtomEnum::CARDINAL.into())?,
    }))
}

/// The instance half of `WM_CLASS` (the first NUL-terminated string,
/// conventionally the lowercase program name — `"code"`, `"firefox"`).
fn wm_class_instance(s: &X11FocusConn, window: Window) -> Result<Option<String>, ()> {
    let cookie = s
        .conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .map_err(|e| debug!(?e, "x11 focus: WM_CLASS send failed"))?;
    match cookie.reply() {
        Ok(reply) => Ok(reply
            .value
            .split(|b| *b == 0)
            .next()
            .filter(|chunk| !chunk.is_empty())
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())),
        Err(ReplyError::X11Error(_)) => Ok(None),
        Err(e) => {
            debug!(?e, "x11 focus: connection error");
            Err(())
        }
    }
}
