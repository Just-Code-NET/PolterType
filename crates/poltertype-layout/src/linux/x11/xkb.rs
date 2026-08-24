//! XKB queries: the layout list, the locked group, and locking a new one.

use super::consts::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId};
use x11rb::connection::Connection;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

/// The core keyboard — the one the user is actually typing on. XKB can
/// address individual devices, but the layout list and locked group we
/// care about live on the core device.
pub(crate) fn core_keyboard() -> xkb::DeviceSpec {
    xkb::ID::USE_CORE_KBD.into()
}

/// Read the configured layout list from the root window.
pub(crate) fn read_layouts(
    conn: &RustConnection,
    root: Window,
) -> Result<Vec<LayoutId>, LayoutError> {
    let atom = conn
        .intern_atom(true, RULES_NAMES_PROPERTY.as_bytes())
        .map_err(|e| LayoutError::Os(format!("InternAtom: {e}")))?
        .reply()
        .map_err(|e| LayoutError::Os(format!("InternAtom reply: {e}")))?
        .atom;
    // `only_if_exists: true` returns None (atom 0) when the server has
    // never set the property — an X server running without XKB rules.
    if atom == x11rb::NONE {
        return Err(LayoutError::Os(format!(
            "{RULES_NAMES_PROPERTY} is not set on the root window"
        )));
    }

    let prop = conn
        .get_property(false, root, atom, AtomEnum::STRING, 0, PROPERTY_LEN)
        .map_err(|e| LayoutError::Os(format!("GetProperty: {e}")))?
        .reply()
        .map_err(|e| LayoutError::Os(format!("GetProperty reply: {e}")))?;

    Ok(parse_rules_names(&prop.value))
}

/// One field of `_XKB_RULES_NAMES` as a string — the property is
/// NUL-separated `rules\0model\0layout\0variant\0options`.
pub(crate) fn read_rules_field(
    conn: &RustConnection,
    root: Window,
    field: usize,
) -> Option<String> {
    let atom = conn
        .intern_atom(true, RULES_NAMES_PROPERTY.as_bytes())
        .ok()?
        .reply()
        .ok()?
        .atom;
    if atom == x11rb::NONE {
        return None;
    }
    let prop = conn
        .get_property(false, root, atom, AtomEnum::STRING, 0, PROPERTY_LEN)
        .ok()?
        .reply()
        .ok()?;
    String::from_utf8_lossy(&prop.value)
        .split('\0')
        .nth(field)
        .map(str::to_owned)
}

/// Pull the layout list out of a raw `_XKB_RULES_NAMES` value:
/// NUL-separated Latin-1, of which only the `layout` field interests us
/// (`us,ua` → `[en-US, uk-UA]`).
///
/// Layouts with no BCP-47 mapping in [`xkb_to_bcp47`] pass through
/// under their raw XKB code rather than being dropped, so an exotic
/// layout still shows up and can still be switched to.
pub(crate) fn parse_rules_names(raw: &[u8]) -> Vec<LayoutId> {
    let text = String::from_utf8_lossy(raw);
    let Some(layouts) = text.split('\0').nth(LAYOUT_FIELD) else {
        return Vec::new();
    };
    layouts
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(|code| {
            LayoutId::new(
                xkb_to_bcp47(code)
                    .map(str::to_owned)
                    .unwrap_or_else(|| code.to_owned()),
            )
        })
        .collect()
}

/// Index of the currently locked XKB group.
pub(crate) fn locked_group(conn: &RustConnection) -> Result<usize, LayoutError> {
    let state = conn
        .xkb_get_state(core_keyboard())
        .map_err(|e| LayoutError::Os(format!("XkbGetState: {e}")))?
        .reply()
        .map_err(|e| LayoutError::Os(format!("XkbGetState reply: {e}")))?;
    Ok(u8::from(state.locked_group).into())
}

/// Lock the keyboard into group `idx`.
pub(crate) fn lock_group(conn: &RustConnection, idx: usize) -> Result<(), LayoutError> {
    let group = group_from_index(idx)?;
    conn.xkb_latch_lock_state(
        core_keyboard(),
        // Touch no modifiers — we are only moving the group.
        0u16.into(),
        0u16.into(),
        true,  // lock_group: make it stick, rather than latch for one key
        group, // the group to lock
        0u16.into(),
        false, // latch_group
        0,     // group_latch
    )
    .map_err(|e| LayoutError::Os(format!("XkbLatchLockState: {e}")))?;
    conn.flush()
        .map_err(|e| LayoutError::Os(format!("x11 flush: {e}")))?;
    Ok(())
}

pub(crate) fn group_from_index(idx: usize) -> Result<xkb::Group, LayoutError> {
    match idx {
        0 => Ok(xkb::Group::M1),
        1 => Ok(xkb::Group::M2),
        2 => Ok(xkb::Group::M3),
        3 => Ok(xkb::Group::M4),
        _ => Err(LayoutError::Os(format!(
            "XKB supports at most {MAX_GROUPS} groups; asked for index {idx}"
        ))),
    }
}
