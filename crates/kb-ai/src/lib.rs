//! Optional AI subsystem: LLM-backed [`Detector`]s and
//! [`WordRewriter`]s.
//!
//! Two extension shims (both already declared in `kb-detect`):
//!
//! * `Detector` — adds another voice to the layout-decision pipeline.
//!   Local ONNX models (`local::LocalOnnxDetector`, stub in v0.1) and
//!   remote LLMs (`remote::RemoteLlmDetector`, gated behind the
//!   `remote` feature) implement it.
//! * `WordRewriter` — operates *after* layout detection on the final
//!   text. Used for power-user tricks like smart-capitalize or
//!   expand-acronym.
//!
//! ## Privacy posture (DECISIONS.md, §3.8 of PLAN.md)
//!
//! * The whole `ai` Cargo feature is **off by default** in `kb-app`.
//! * The `remote` cargo sub-feature is **off by default** even when
//!   `ai` is on; it adds `reqwest` and TLS to the build.
//! * Even when `ai = enabled` and `remote` is built in, the
//!   `[ai].allow_remote` settings flag must also be true at runtime.
//!   Two switches by design.
//! * API keys are stored in the OS keychain via `keyring`, never in
//!   `config.toml`.

#![forbid(unsafe_code)]

pub mod local;
pub mod remote;
pub mod rewriters;

pub use kb_detect::{Detector, RewriteRequest, RewriteVerdict, Verdict, WordRewriter};

use serde::{Deserialize, Serialize};
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
}

/// Resolve an API key reference (e.g. `"keyring:anthropic"`) into the
/// actual secret via the OS keychain.
pub fn resolve_api_key(reference: &str) -> Result<String, AiError> {
    let Some(rest) = reference.strip_prefix("keyring:") else {
        return Err(AiError::KeyringLookup(
            reference.to_owned(),
            "expected 'keyring:<entry-name>' reference".into(),
        ));
    };
    let entry = keyring::Entry::new("kb-switcher", rest)
        .map_err(|e| AiError::KeyringLookup(rest.to_owned(), e.to_string()))?;
    entry
        .get_password()
        .map_err(|e| AiError::KeyringLookup(rest.to_owned(), e.to_string()))
}

/// Per-detector / per-rewriter configuration as it appears in
/// `config.toml`. The kb-ai subsystem will iterate this list and
/// instantiate concrete plug-ins from each entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AiPluginConfig {
    pub r#type: String,
    pub id: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_path: Option<std::path::PathBuf>,
    #[serde(default)]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub max_latency_ms: Option<u64>,
    #[serde(default)]
    pub require_confirmation: Option<bool>,
    #[serde(default)]
    pub weight: Option<f32>,
}
