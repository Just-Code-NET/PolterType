//! The portal's request/response handshake: subscribing to `Response`
//! signals before issuing a call (see the module docs for why the
//! order matters), waiting for the matching one, and the path-safe
//! handle tokens every call needs.

use std::collections::HashMap;
use std::time::Duration;

use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{ObjectPath, OwnedValue};
use zbus::{MatchRule, message};

use super::consts::*;
use super::enums::PortalError;

/// Subscribe to `Response` before making a call — see the module docs
/// for why the order matters.
pub(super) fn subscribe(
    conn: &Connection,
    call: &'static str,
) -> Result<MessageIterator, PortalError> {
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

pub(super) fn wait_response(
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
pub(super) fn request_path_of(
    reply: &zbus::Message,
    call: &'static str,
) -> Result<String, PortalError> {
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
pub(super) fn unique_token(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{prefix}_{pid}_{n}")
}

#[cfg(test)]
pub(super) fn token_for_test(prefix: &str) -> String {
    unique_token(prefix)
}
