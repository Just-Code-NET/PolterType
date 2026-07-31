//! AI subsystem errors.

pub use poltertype_detect::{Detector, RewriteRequest, RewriteVerdict, Verdict, WordRewriter};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("keyring lookup for {0:?} failed: {1}")]
    KeyringLookup(String, String),
    #[error("model file not found at {0}")]
    ModelMissing(std::path::PathBuf),
    #[cfg(feature = "remote")]
    #[error("remote LLM call failed: {0}")]
    Remote(#[from] reqwest::Error),
    #[error("remote LLM disabled: {0}")]
    RemoteDisabled(String),
    /// The `[[ai.plugins]]` entry does not describe a buildable
    /// plug-in. Always names the entry's `id` at the call site, so a
    /// user with three plug-ins learns which one is wrong.
    #[error("invalid plug-in config: {0}")]
    Config(String),
}
