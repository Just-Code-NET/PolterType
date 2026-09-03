//! Who a caret sample belongs to.
//!
//! A `TextCaretMoved` signal says where a caret is, never whose it is,
//! and the consumer composes those window-relative coordinates with the
//! *focused* window's rect. Two answers keep that composition honest:
//! the process behind the sending bus connection, and the size of the
//! toplevel the coordinates are measured against. Both are cached —
//! caret events fire on every keystroke, while the answers change only
//! when the user moves to another text field.
//!
//! PRIVACY: identity and geometry only — PIDs, object paths and
//! rectangles. Never an accessible name, a window title or text.

use std::collections::HashMap;

use tracing::debug;
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

use super::atspi_caret_queries::object_extents;
use super::types::CaretOwner;

/// The path every AT-SPI application registers its own root object at,
/// and therefore the marker that a walk up the parent chain has left
/// the window it started in.
const APP_ROOT: &str = "/org/a11y/atspi/accessible/root";
/// The "no such object" path AT-SPI answers with instead of an error.
const NULL_PATH: &str = "/org/a11y/atspi/null";
/// Parent hops before the walk gives up. Web-based apps nest deeply —
/// a VS Code editor caret sits 36 levels below its window — and a cap
/// short of the real depth costs a whole toolkit its window identity
/// with nothing to show for it, so this is slack over the worst
/// measured rather than a tight bound.
const MAX_PARENT_HOPS: usize = 96;
/// Bus names remembered. Unique names are never reused within one bus
/// lifetime, so entries stay valid; the cap only stops a long session
/// full of short-lived apps from growing without end.
const PID_CACHE_CAP: usize = 128;

/// Identity resolver plus its caches. Lives on the caret watcher
/// thread and is never shared.
#[derive(Default)]
pub(crate) struct OwnerLookup {
    pids: HashMap<String, u32>,
    /// Last object → toplevel resolution. One entry, because a burst of
    /// caret events is one field being typed into; moving to another
    /// field simply pays for one more walk.
    toplevel: Option<(String, String, OwnedObjectPath)>,
}

impl OwnerLookup {
    /// Identify the sender and object of one caret signal. `None` means
    /// the sender could not be tied to a process, which makes the
    /// sample unusable: nothing else can then prove it belongs to the
    /// window the tooltip is about to anchor to.
    pub(crate) fn resolve(
        &mut self,
        conn: &Connection,
        sender: &str,
        path: &ObjectPath<'_>,
    ) -> Option<CaretOwner> {
        let pid = self.pid(conn, sender)?;
        let window = self
            .toplevel(conn, sender, path)
            .and_then(|top| window_size(conn, sender, &top));
        Some(CaretOwner { pid, window })
    }

    fn pid(&mut self, conn: &Connection, sender: &str) -> Option<u32> {
        if let Some(pid) = self.pids.get(sender) {
            return Some(*pid);
        }
        let pid = connection_pid(conn, sender)?;
        if self.pids.len() >= PID_CACHE_CAP {
            self.pids.clear();
        }
        self.pids.insert(sender.to_owned(), pid);
        Some(pid)
    }

    fn toplevel(
        &mut self,
        conn: &Connection,
        sender: &str,
        path: &ObjectPath<'_>,
    ) -> Option<OwnedObjectPath> {
        if let Some((s, p, top)) = &self.toplevel
            && s == sender
            && p == path.as_str()
        {
            return Some(top.clone());
        }
        let top = walk_to_toplevel(conn, sender, path)?;
        self.toplevel = Some((sender.to_owned(), path.to_string(), top.clone()));
        Some(top)
    }
}

/// Ask the a11y bus which process owns a connection. The app talks to
/// that bus itself, so the daemon knows its PID and will say — no
/// compositor, no extension, no user-installed script.
pub(crate) fn connection_pid(conn: &Connection, sender: &str) -> Option<u32> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &(sender,),
        )
        .map_err(|e| debug!(%e, "AT-SPI: PID lookup failed"))
        .ok()?;
    reply.body().deserialize::<u32>().ok()
}

/// Climb the `Parent` chain until the next step would leave the
/// application, and answer with the object below it — the toplevel
/// window, which is what `COORD_TYPE_WINDOW` measures against.
fn walk_to_toplevel(
    conn: &Connection,
    sender: &str,
    path: &ObjectPath<'_>,
) -> Option<OwnedObjectPath> {
    let mut current: OwnedObjectPath = path.to_owned().into();
    for _ in 0..MAX_PARENT_HOPS {
        let parent = parent_path(conn, sender, &current)?;
        if parent.as_str() == APP_ROOT || parent.as_str() == NULL_PATH {
            return Some(current);
        }
        current = parent;
    }
    debug!("AT-SPI: parent chain too deep to find a toplevel");
    None
}

/// The `Parent` property: a `(bus_name, object_path)` pair inside a
/// variant. Only the path is used — a parent always lives on the
/// sender's own connection.
fn parent_path(conn: &Connection, sender: &str, path: &ObjectPath<'_>) -> Option<OwnedObjectPath> {
    let reply = conn
        .call_method(
            Some(sender),
            path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.a11y.atspi.Accessible", "Parent"),
        )
        .map_err(|e| debug!(%e, "AT-SPI: parent lookup failed"))
        .ok()?;
    let body = reply.body();
    let Ok(Value::Structure(parent)) = body.deserialize::<Value<'_>>() else {
        return None;
    };
    match parent.fields() {
        [_, Value::ObjectPath(p)] => Some(p.clone().into()),
        _ => None,
    }
}

/// The toplevel's own width and height, read from its window-relative
/// extents — `(0, 0, w, h)` for every toolkit measured. A degenerate
/// answer is no answer: it would match no real window.
fn window_size(conn: &Connection, sender: &str, path: &ObjectPath<'_>) -> Option<(u32, u32)> {
    let (_, _, w, h) = object_extents(conn, sender, path)?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((u32::try_from(w).ok()?, u32::try_from(h).ok()?))
}
