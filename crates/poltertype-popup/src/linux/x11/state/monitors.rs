//! Which physical monitor the popup belongs on.
//!
//! Root coordinates span the whole desktop: with two monitors side by
//! side the X11 root is as wide as both, so a popup centred in it lands
//! on the seam and is drawn half on each screen — what
//! [#70](https://github.com/Just-Code-NET/PolterType/issues/70) reports
//! on GNOME, where Mutter offers no layer shell and this backend is the
//! only one available.
//!
//! RandR 1.5 reports the physical rectangles. Everything here answers
//! "which of them", and [`super::show`] then measures inside that one.
//! Every query is best-effort: a server without RandR 1.5, or one that
//! answers nothing, leaves the whole root as the single rectangle —
//! exactly the placement this file replaces.

use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus, Window};
use x11rb::rust_connection::RustConnection;

use crate::enums::PopupAnchor;

/// A monitor's rectangle in root coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MonitorRect {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) w: i32,
    pub(super) h: i32,
}

/// The monitor `anchor` belongs on. `root_rect` is the whole-screen
/// fallback, returned when the server names no monitors at all.
///
/// Costs one round trip on a single-monitor machine and at most three
/// on a multi-head one, all on the popup's own thread and only while a
/// popup is being shown. Asked per show rather than cached because a
/// cached list is wrong for exactly as long as nobody replugs a
/// monitor, and there is no cheaper way to learn that they did.
pub(super) fn for_anchor(
    conn: &RustConnection,
    root: Window,
    root_rect: MonitorRect,
    anchor: &PopupAnchor,
) -> MonitorRect {
    let mons = list(conn, root);
    match mons[..] {
        [] => return root_rect,
        [only] => return only,
        _ => {}
    }
    let point = match *anchor {
        PopupAnchor::Point { x, y, .. } => Some((x, y)),
        PopupAnchor::WindowRect {
            x,
            y,
            width,
            height,
            ..
        } => Some((x + width as i32 / 2, y + height as i32 / 2)),
        // Nothing in the anchor says where the text is, so ask the
        // server where the user is.
        PopupAnchor::ScreenBottom => typing_point(conn, root),
    };
    point
        .and_then(|(x, y)| containing(&mons, x, y))
        .or_else(|| mons.first().copied())
        .unwrap_or(root_rect)
}

/// The monitor a root-space point falls on, if any. Monitors may
/// overlap (mirrored outputs); the first match wins, which is the
/// server's own order and puts the primary first on every desktop
/// that sets one.
pub(super) fn containing(mons: &[MonitorRect], x: i32, y: i32) -> Option<MonitorRect> {
    mons.iter()
        .copied()
        .find(|m| x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h)
}

fn list(conn: &RustConnection, root: Window) -> Vec<MonitorRect> {
    let Ok(cookie) = conn.randr_get_monitors(root, true) else {
        return Vec::new();
    };
    let Ok(reply) = cookie.reply() else {
        return Vec::new();
    };
    reply
        .monitors
        .iter()
        .map(|m| MonitorRect {
            x: m.x.into(),
            y: m.y.into(),
            w: m.width.into(),
            h: m.height.into(),
        })
        .collect()
}

/// Where the user most likely is: the focused window's centre, else
/// the pointer.
///
/// The focus comes first because a keyboard app should follow the
/// keyboard — the mouse is often parked on the other screen. It is also
/// the half that is often unavailable: under XWayland only X clients
/// hold the X input focus, so a native Wayland window leaves us the
/// pointer, which is why both are asked.
fn typing_point(conn: &RustConnection, root: Window) -> Option<(i32, i32)> {
    focus_point(conn, root).or_else(|| pointer_point(conn, root))
}

fn focus_point(conn: &RustConnection, root: Window) -> Option<(i32, i32)> {
    let focus = conn.get_input_focus().ok()?.reply().ok()?.focus;
    if focus == x11rb::NONE || focus == u32::from(InputFocus::POINTER_ROOT) || focus == root {
        return None;
    }
    let geom = conn.get_geometry(focus).ok()?.reply().ok()?;
    // `get_geometry` answers in the parent's space, and a reparenting
    // window manager makes that a frame rather than the root.
    let at = conn
        .translate_coordinates(focus, root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    Some((
        i32::from(at.dst_x) + i32::from(geom.width) / 2,
        i32::from(at.dst_y) + i32::from(geom.height) / 2,
    ))
}

fn pointer_point(conn: &RustConnection, root: Window) -> Option<(i32, i32)> {
    let p = conn.query_pointer(root).ok()?.reply().ok()?;
    // False when the pointer is on another X screen entirely, where
    // its coordinates mean nothing here.
    p.same_screen
        .then_some((i32::from(p.root_x), i32::from(p.root_y)))
}
