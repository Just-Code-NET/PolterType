//! AT-SPI2 D-Bus queries against one accessible object: glyph and
//! object extents, and the `CaretOffset` property fallback. Every
//! failure here (app exited, object destroyed, interface missing) is
//! normal churn, so all three degrade to `None` rather than propagate
//! an error.

use tracing::debug;
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, Value};

use super::atspi_caret::Extents;
use super::consts::COORD_TYPE_WINDOW;

/// `GetCharacterExtents` on the signal's accessible, in [window
/// coordinates](COORD_TYPE_WINDOW). Failures (app exited, object
/// destroyed, interface not implemented) are normal churn — `None`,
/// logged at debug.
pub(super) fn character_extents(
    conn: &Connection,
    sender: &str,
    path: &ObjectPath<'_>,
    offset: i32,
) -> Option<Extents> {
    let reply = conn
        .call_method(
            Some(sender),
            path.clone(),
            Some("org.a11y.atspi.Text"),
            "GetCharacterExtents",
            &(offset, COORD_TYPE_WINDOW),
        )
        .map_err(|e| debug!(%e, "AT-SPI caret watcher: GetCharacterExtents failed"))
        .ok()?;
    reply.body().deserialize::<Extents>().ok()
}

/// The accessible's own rectangle, in the same [window
/// coordinates](COORD_TYPE_WINDOW) as [`character_extents`].
pub(super) fn object_extents(
    conn: &Connection,
    sender: &str,
    path: &ObjectPath<'_>,
) -> Option<Extents> {
    let reply = conn
        .call_method(
            Some(sender),
            path.clone(),
            Some("org.a11y.atspi.Component"),
            "GetExtents",
            &(COORD_TYPE_WINDOW,),
        )
        .map_err(|e| debug!(%e, "AT-SPI caret watcher: GetExtents failed"))
        .ok()?;
    reply.body().deserialize::<Extents>().ok()
}

/// The object's current caret offset, via the `CaretOffset` property.
/// (libatspi's `atspi_text_get_caret_offset` maps to this property —
/// there is no `GetCaretOffset` *method* on the wire.)
pub(super) fn caret_offset_property(
    conn: &Connection,
    sender: &str,
    path: &ObjectPath<'_>,
) -> Option<i32> {
    let reply = conn
        .call_method(
            Some(sender),
            path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.a11y.atspi.Text", "CaretOffset"),
        )
        .map_err(|e| debug!(%e, "AT-SPI caret watcher: CaretOffset query failed"))
        .ok()?;
    match reply.body().deserialize::<Value<'_>>().ok()? {
        Value::I32(offset) => Some(offset),
        _ => None,
    }
}
