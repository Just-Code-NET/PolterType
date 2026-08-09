//! Redaction of user-typed text in logs and decision reasons.
//!
//! The privacy contract is that PolterType never logs what the user
//! types, so every diagnostic that wants to name a word routes it
//! through [`redact_word`]. That only ever reveals it in a **debug
//! build** where the developer set `POLTERTYPE_UNSAFE_LOG_WORDS=1`;
//! release builds redact at compile time, and no configuration can
//! reveal typed text there.

use std::sync::OnceLock;

/// Render `word` for a log line or decision reason: `` `word` ``
/// (backticks included) in an explicitly opted-in debug session,
/// `<N chars>` everywhere else. The length still gives a debug log
/// its diagnostic shape ("did the buffer hold the whole word?")
/// without giving away the word.
pub fn redact_word(word: &str) -> String {
    render(word, words_allowed())
}

/// The gate: compile-time `debug_assertions` AND the explicit env
/// opt-in. Read once — flipping the variable mid-run is not a
/// supported way to toggle logging.
fn words_allowed() -> bool {
    static ALLOWED: OnceLock<bool> = OnceLock::new();
    *ALLOWED.get_or_init(|| {
        cfg!(debug_assertions) && std::env::var("POLTERTYPE_UNSAFE_LOG_WORDS").as_deref() == Ok("1")
    })
}

pub(crate) fn render(word: &str, allowed: bool) -> String {
    if allowed {
        format!("`{word}`")
    } else {
        format!("<{} chars>", word.chars().count())
    }
}
