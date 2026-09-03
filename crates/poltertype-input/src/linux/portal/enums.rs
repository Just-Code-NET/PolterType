//! Failure modes of a portal call.

use super::consts::RESPONSE_TIMEOUT_SECS;

#[derive(Debug, thiserror::Error)]
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
