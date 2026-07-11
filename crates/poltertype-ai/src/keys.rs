//! Keyring-backed API-key resolution (never plain text).

use crate::*;

/// Resolve an API key reference (e.g. `"keyring:anthropic"`) into the
/// actual secret via the OS keychain.
pub fn resolve_api_key(reference: &str) -> Result<String, AiError> {
    let Some(rest) = reference.strip_prefix("keyring:") else {
        return Err(AiError::KeyringLookup(
            reference.to_owned(),
            "expected 'keyring:<entry-name>' reference".into(),
        ));
    };
    let entry = keyring::Entry::new("poltertype", rest)
        .map_err(|e| AiError::KeyringLookup(rest.to_owned(), e.to_string()))?;
    entry
        .get_password()
        .map_err(|e| AiError::KeyringLookup(rest.to_owned(), e.to_string()))
}
