//! Turning a question into a request body, and a response into an
//! answer, for each supported endpoint shape.
//!
//! Free of any HTTP type on purpose: bodies are plain strings, so the
//! whole contract is unit-testable even where the `remote` feature is
//! off and `reqwest` is not compiled at all.

use crate::consts::{ANTHROPIC_VERSION, SYSTEM_PROMPT};
use crate::enums::WireFormat;

/// One question: the candidate readings, in the order they were
/// offered. The reply is an index into this list (1-based), or 0.
pub struct Question<'a> {
    pub model: &'a str,
    pub candidates: &'a [String],
}

impl Question<'_> {
    /// The user-visible half of the prompt: only the candidate strings
    /// — no surrounding text, no application name, and no layout ids,
    /// which would leak the languages the user has installed.
    pub fn prompt(&self) -> String {
        let mut s = String::with_capacity(32 + self.candidates.len() * 16);
        for (i, cand) in self.candidates.iter().enumerate() {
            s.push_str(&format!("{}. {cand}\n", i + 1));
        }
        s
    }
}

/// JSON-encode a string value, including the surrounding quotes.
///
/// Hand-rolled to keep `serde_json` out of the dependency tree. Escapes
/// what RFC 8259 requires: quote, backslash, everything below U+0020.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Build the POST body for `format`.
pub fn request_body(format: WireFormat, q: &Question<'_>) -> String {
    let model = json_str(q.model);
    let prompt = json_str(&q.prompt());
    let system = json_str(SYSTEM_PROMPT);
    match format {
        WireFormat::OpenAiChat => format!(
            r#"{{"model":{model},"max_tokens":4,"temperature":0,"messages":[{{"role":"system","content":{system}}},{{"role":"user","content":{prompt}}}]}}"#
        ),
        WireFormat::AnthropicMessages => format!(
            r#"{{"model":{model},"max_tokens":4,"temperature":0,"system":{system},"messages":[{{"role":"user","content":{prompt}}}]}}"#
        ),
        WireFormat::OllamaGenerate => {
            // Ollama's native API takes one prompt string and needs
            // `stream:false` or it replies with newline-delimited
            // chunks that our single-shot parse would not survive.
            let joined = json_str(&format!("{SYSTEM_PROMPT}\n\n{}", q.prompt()));
            format!(
                r#"{{"model":{model},"prompt":{joined},"stream":false,"options":{{"temperature":0,"num_predict":4}}}}"#
            )
        }
    }
}

/// Headers beyond `content-type`, as `(name, value)` pairs.
pub fn headers(format: WireFormat, api_key: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(key) = api_key else {
        return out;
    };
    match format {
        WireFormat::AnthropicMessages => {
            out.push(("x-api-key".into(), key.to_owned()));
            out.push(("anthropic-version".into(), ANTHROPIC_VERSION.into()));
        }
        // Ollama ignores auth locally but proxies in front of it often
        // don't, so send the bearer if the user configured one.
        WireFormat::OpenAiChat | WireFormat::OllamaGenerate => {
            out.push(("authorization".into(), format!("Bearer {key}")));
        }
    }
    out
}

/// Pull the model's answer text out of a response body.
///
/// **Not a JSON parser**: it finds the one key each format puts the
/// text under, then reads the following JSON string with escape
/// handling. An unexpected shape yields `None`, which the caller turns
/// into "no opinion" like any other failure.
pub fn extract_text(format: WireFormat, body: &str) -> Option<String> {
    let key = match format {
        WireFormat::OpenAiChat => "\"content\"",
        WireFormat::AnthropicMessages => "\"text\"",
        WireFormat::OllamaGenerate => "\"response\"",
    };
    let idx = body.find(key)?;
    let after = &body[idx + key.len()..];
    let colon = after.find(':')?;
    read_json_string(&after[colon + 1..])
}

/// Read one JSON string starting at (or before, skipping whitespace)
/// the opening quote.
fn read_json_string(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let mut out = String::new();
    let mut chars = s[start + 1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let cp = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Interpret the model's reply as a 1-based index into the candidate
/// list, or `None` for "none of them" and anything unparseable.
///
/// Tolerant of the ways a model garnishes a number, strict about the
/// result: the first run of digits must name a candidate that exists.
pub fn parse_choice(reply: &str, candidate_count: usize) -> Option<usize> {
    let trimmed = reply.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let first_digit = chars.iter().position(char::is_ascii_digit)?;

    // A model means "none of these" by a negative number. Skipping the
    // sign would read `-1` as candidate 1 and retype the user's word as
    // something they did not ask for.
    if first_digit > 0 && chars[first_digit - 1] == '-' {
        return None;
    }

    let digits: String = chars[first_digit..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let n: usize = digits.parse().ok()?;
    if n == 0 || n > candidate_count {
        return None;
    }
    Some(n - 1)
}

#[cfg(test)]
mod tests;
