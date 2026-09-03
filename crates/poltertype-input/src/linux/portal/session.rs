//! Negotiating a RemoteDesktop session.
//!
//! Every portal call is asynchronous in the same shape: the method
//! returns a `Request` object path immediately, and the actual answer
//! arrives later as a `Response` signal on that path. So each step
//! here subscribes *before* calling — a portal that answers quickly
//! would otherwise deliver the signal before we were listening, and
//! the call would hang until the timeout for no reason.

use std::collections::HashMap;

use tracing::{debug, info};
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, Value};

use super::consts::*;
use super::enums::PortalError;
use super::response::{request_path_of, subscribe, unique_token, wait_response};
use super::restore_token::{load_restore_token, store_restore_token};

/// Is a RemoteDesktop portal present at all?
///
/// Cheap enough to call before deciding on a backend: it reads one
/// property. A session without the interface (wlroots today) answers
/// no, and the caller stays on `uinput`.
pub fn portal_available() -> bool {
    let Ok(conn) = Connection::session() else {
        return false;
    };
    conn.call_method(
        Some(PORTAL_BUS),
        PORTAL_PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(REMOTE_DESKTOP_IFACE, "version"),
    )
    .is_ok()
}

/// A negotiated session: the portal has granted keyboard emulation
/// and will accept `NotifyKeyboardKeycode` on this handle.
pub struct PortalSession {
    conn: Connection,
    handle: String,
}

impl PortalSession {
    /// Create, configure and start a session. Blocks — `Start` shows
    /// the consent dialog, so this must never be called from the
    /// correction path. The caller does it once, at startup.
    pub fn open() -> Result<Self, PortalError> {
        let conn = Connection::session().map_err(PortalError::Bus)?;
        if !portal_available() {
            return Err(PortalError::NotAvailable);
        }

        let session_token = unique_token("pt_session");
        let session_handle = create_session(&conn, &session_token)?;
        select_devices(&conn, &session_handle)?;
        let restore_token = start(&conn, &session_handle)?;

        if let Some(token) = restore_token {
            store_restore_token(&token);
        }
        info!(
            "RemoteDesktop portal session established — PolterType can type without \
             input-group membership on this session"
        );
        Ok(Self {
            conn,
            handle: session_handle,
        })
    }

    /// Press or release one evdev keycode.
    ///
    /// **Evdev codes, not X11 keycodes**: the portal takes the code
    /// the kernel uses (`KEY_A` = 30), the same numbering the rest of
    /// this crate speaks, with no `+8` offset anywhere.
    pub fn notify_keycode(&self, keycode: i32, pressed: bool) -> Result<(), PortalError> {
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let state = if pressed { KEY_PRESSED } else { KEY_RELEASED };
        self.conn
            .call_method(
                Some(PORTAL_BUS),
                PORTAL_PATH,
                Some(REMOTE_DESKTOP_IFACE),
                "NotifyKeyboardKeycode",
                &(
                    ObjectPath::try_from(self.handle.as_str())
                        .map_err(|_| PortalError::BadReply("NotifyKeyboardKeycode"))?,
                    options,
                    keycode,
                    state,
                ),
            )
            .map_err(|source| PortalError::Call {
                call: "NotifyKeyboardKeycode",
                source,
            })?;
        Ok(())
    }
}

impl Drop for PortalSession {
    /// Close the session so the compositor drops its "an app is
    /// controlling this screen" indicator rather than leaving it up
    /// until the bus connection dies.
    fn drop(&mut self) {
        if let Ok(path) = ObjectPath::try_from(self.handle.as_str()) {
            let _ =
                self.conn
                    .call_method(Some(PORTAL_BUS), path, Some(SESSION_IFACE), "Close", &());
        }
    }
}

fn create_session(conn: &Connection, session_token: &str) -> Result<String, PortalError> {
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    let handle_token = unique_token("pt_req");
    options.insert("handle_token", Value::from(handle_token.as_str()));
    options.insert("session_handle_token", Value::from(session_token));

    let call = "CreateSession";
    let mut signals = subscribe(conn, call)?;
    let reply = conn
        .call_method(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            Some(REMOTE_DESKTOP_IFACE),
            call,
            &(&options,),
        )
        .map_err(|source| PortalError::Call { call, source })?;
    let path = request_path_of(&reply, call)?;
    let results = wait_response(call, &mut signals, &path)?;
    results
        .get("session_handle")
        .and_then(|v| String::try_from(v.clone()).ok())
        .ok_or(PortalError::BadReply("CreateSession"))
}

fn select_devices(conn: &Connection, session_handle: &str) -> Result<(), PortalError> {
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    let handle_token = unique_token("pt_req");
    options.insert("handle_token", Value::from(handle_token.as_str()));
    options.insert("types", Value::from(DEVICE_KEYBOARD));
    options.insert("persist_mode", Value::from(PERSIST_PERSISTENT));
    // A token from a previous grant, if the compositor gave us one —
    // this is what stops the dialog appearing on every launch.
    let restored = load_restore_token();
    if let Some(token) = &restored {
        debug!("reusing a stored portal restore token");
        options.insert("restore_token", Value::from(token.as_str()));
    }

    let call = "SelectDevices";
    let session = ObjectPath::try_from(session_handle).map_err(|_| PortalError::BadReply(call))?;
    let mut signals = subscribe(conn, call)?;
    let reply = conn
        .call_method(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            Some(REMOTE_DESKTOP_IFACE),
            call,
            &(session, &options),
        )
        .map_err(|source| PortalError::Call { call, source })?;
    let path = request_path_of(&reply, call)?;
    wait_response(call, &mut signals, &path)?;
    Ok(())
}

/// `Start` is the one that prompts. Returns the restore token when
/// the compositor issued one.
fn start(conn: &Connection, session_handle: &str) -> Result<Option<String>, PortalError> {
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    let handle_token = unique_token("pt_req");
    options.insert("handle_token", Value::from(handle_token.as_str()));

    let call = "Start";
    let session = ObjectPath::try_from(session_handle).map_err(|_| PortalError::BadReply(call))?;
    let mut signals = subscribe(conn, call)?;
    // Empty parent window: PolterType is a tray app and has no window
    // to parent the dialog to.
    let reply = conn
        .call_method(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            Some(REMOTE_DESKTOP_IFACE),
            call,
            &(session, "", &options),
        )
        .map_err(|source| PortalError::Call { call, source })?;
    let path = request_path_of(&reply, call)?;
    let results = wait_response(call, &mut signals, &path)?;
    Ok(results
        .get("restore_token")
        .and_then(|v| String::try_from(v.clone()).ok()))
}
