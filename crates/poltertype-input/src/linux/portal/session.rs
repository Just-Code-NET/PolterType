//! Negotiating a RemoteDesktop session.
//!
//! Every portal call is asynchronous in the same shape: the method
//! returns a `Request` object path immediately, and the actual answer
//! arrives later as a `Response` signal on that path. So each step
//! here subscribes *before* calling — a portal that answers quickly
//! would otherwise deliver the signal before we were listening, and
//! the call would hang until the timeout for no reason.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;
use tracing::{debug, info, warn};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{MatchRule, message};

use super::consts::*;

#[derive(Debug, Error)]
pub enum PortalError {
    #[error("session bus unavailable: {0}")]
    Bus(zbus::Error),
    #[error("the RemoteDesktop portal is not available on this session")]
    NotAvailable,
    #[error("portal call {call} failed: {source}")]
    Call {
        call: &'static str,
        #[source]
        source: zbus::Error,
    },
    #[error("portal call {0} timed out after {RESPONSE_TIMEOUT_SECS}s")]
    Timeout(&'static str),
    #[error("the user declined the screen-sharing prompt")]
    Cancelled,
    #[error("portal call {call} returned response code {code}")]
    Refused { call: &'static str, code: u32 },
    #[error("portal reply had an unexpected shape during {0}")]
    BadReply(&'static str),
}

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

/// Subscribe to `Response` before making a call.
///
/// Order matters: a portal that answers immediately would emit the
/// signal before we were listening, and we would then wait out the
/// whole timeout for something already delivered.
fn subscribe(conn: &Connection, call: &'static str) -> Result<MessageIterator, PortalError> {
    let rule = MatchRule::builder()
        .msg_type(message::Type::Signal)
        .interface(REQUEST_IFACE)
        .map_err(|source| PortalError::Call { call, source })?
        .member("Response")
        .map_err(|source| PortalError::Call { call, source })?
        .build();
    MessageIterator::for_match_rule(rule, conn, Some(8))
        .map_err(|source| PortalError::Call { call, source })
}

/// Wait for the `Response` belonging to `request_path`.
fn wait_response(
    call: &'static str,
    signals: &mut MessageIterator,
    request_path: &str,
) -> Result<HashMap<String, OwnedValue>, PortalError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(RESPONSE_TIMEOUT_SECS);
    while std::time::Instant::now() < deadline {
        let Some(Ok(msg)) = signals.next() else {
            break;
        };
        let header = msg.header();
        if header.path().map(ToString::to_string).as_deref() != Some(request_path) {
            continue; // somebody else's request
        }
        let (code, results): (u32, HashMap<String, OwnedValue>) = msg
            .body()
            .deserialize()
            .map_err(|_| PortalError::BadReply(call))?;
        return match code {
            RESPONSE_SUCCESS => Ok(results),
            RESPONSE_CANCELLED => Err(PortalError::Cancelled),
            other => Err(PortalError::Refused { call, code: other }),
        };
    }
    Err(PortalError::Timeout(call))
}

/// The `Request` object path a portal call returns.
fn request_path_of(reply: &zbus::Message, call: &'static str) -> Result<String, PortalError> {
    // Bind the body: deserialising borrows from it, so a temporary
    // would be dropped while the ObjectPath still points into it.
    let body = reply.body();
    let path: ObjectPath<'_> = body
        .deserialize()
        .map_err(|_| PortalError::BadReply(call))?;
    Ok(path.to_string())
}

/// Portal handle tokens must be unique per connection and are
/// restricted to `[A-Za-z0-9_]`.
fn unique_token(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{prefix}_{pid}_{n}")
}

fn restore_token_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("dev", "opensource", "poltertype")
        .map(|d| d.data_local_dir().join(RESTORE_TOKEN_FILE))
}

fn load_restore_token() -> Option<String> {
    let path = restore_token_path()?;
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

/// Store the token so the next launch is silent.
///
/// Best-effort: a machine where this cannot be written just prompts
/// again next time, which is annoying rather than broken.
fn store_restore_token(token: &str) {
    let Some(path) = restore_token_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, token) {
        warn!(path = %path.display(), %e, "could not store the portal restore token");
    }
}

/// Test-only view of the token generator.
#[cfg(test)]
pub(super) fn token_for_test(prefix: &str) -> String {
    unique_token(prefix)
}
