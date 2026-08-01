//! The one place in this crate that opens a socket.
//!
//! Isolated behind the `remote` cargo feature so that a default build
//! contains no HTTP client at all — not a disabled one, not one behind
//! a runtime flag. `cargo tree` on a stock build shows no `reqwest`,
//! which is a stronger statement than any amount of documentation
//! about what the app does not do.

use crate::AiError;
use crate::enums::WireFormat;
use crate::wire;

/// Everything one query needs, resolved and validated.
pub struct Call<'a> {
    pub endpoint: &'a str,
    pub format: WireFormat,
    pub model: &'a str,
    pub api_key: Option<&'a str>,
    pub candidates: &'a [String],
}

/// Perform one query and return the model's chosen candidate index.
///
/// `Ok(None)` means the model declined to pick — a legitimate answer,
/// cached like any other. `Err` means the call itself failed and
/// nothing should be remembered.
#[cfg(feature = "remote")]
pub fn ask(client: &reqwest::blocking::Client, call: &Call<'_>) -> Result<Option<usize>, AiError> {
    let question = wire::Question {
        model: call.model,
        candidates: call.candidates,
    };
    let body = wire::request_body(call.format, &question);

    let mut req = client
        .post(call.endpoint)
        .header("content-type", "application/json");
    for (name, value) in wire::headers(call.format, call.api_key) {
        req = req.header(name, value);
    }

    let resp = req.body(body).send()?;
    // A non-2xx is not an exception here — a wrong key or a model name
    // the server doesn't know is an ordinary misconfiguration. Report
    // it as a failure so the caller can announce it once, and make
    // sure the body (which may quote the request) never reaches a log.
    if !resp.status().is_success() {
        return Err(AiError::RemoteDisabled(format!(
            "endpoint returned HTTP {}",
            resp.status().as_u16()
        )));
    }
    let text = resp.text()?;
    let Some(reply) = wire::extract_text(call.format, &text) else {
        return Err(AiError::RemoteDisabled(
            "response did not contain the expected field — is `format` right for this endpoint?"
                .into(),
        ));
    };
    Ok(wire::parse_choice(&reply, call.candidates.len()))
}

/// Without the `remote` feature there is no client type to take, and
/// the detector never constructs one — this exists so the module
/// compiles and any accidental call site fails loudly.
#[cfg(not(feature = "remote"))]
pub fn ask(_call: &Call<'_>) -> Result<Option<usize>, AiError> {
    Err(AiError::RemoteDisabled(
        "built without the `remote` cargo feature — no HTTP client exists in this binary".into(),
    ))
}
