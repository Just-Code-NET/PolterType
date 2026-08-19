//! The one place in this crate that opens a socket.
//!
//! Isolated behind the `remote` cargo feature so a default build
//! contains no HTTP client at all — not a disabled one, not one behind
//! a runtime flag. `cargo tree` on a stock build shows no `reqwest`,
//! which is a stronger claim than documentation can make.

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
    // A non-2xx is ordinary misconfiguration (wrong key, unknown model
    // name), reported so the caller announces it once. The body may
    // quote the request, so it must never reach a log.
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
