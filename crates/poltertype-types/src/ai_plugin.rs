//! Declaration of one AI plug-in, as it appears in `config.toml`.
//!
//! Lives here rather than in `poltertype-ai` because two crates that
//! cannot see each other both need it: `poltertype-core` parses it as
//! part of the settings file, and `poltertype-ai` turns it into a
//! detector. `poltertype-types` is the only place both already depend
//! on — and keeping the schema out of the optional crate means a build
//! *without* the `ai` feature still parses an `[[ai.plugins]]` entry
//! instead of rejecting the user's config file.

use serde::{Deserialize, Serialize};

/// One `[[ai.plugins]]` entry.
///
/// Deliberately one flat struct rather than a tagged enum: `type` is a
/// plain string so that a config naming a plug-in kind this build does
/// not know is *reported and skipped*, not a parse error that takes
/// the whole settings file down with it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AiPluginConfig {
    /// Which backend to construct: `local-onnx` or `remote-llm`.
    pub r#type: String,
    /// Stable identifier, used in logs and to tell two entries of the
    /// same kind apart.
    pub id: String,

    // ── remote-llm ────────────────────────────────────────────────
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// `keyring:<entry>` — never the key itself. A literal secret here
    /// is rejected at construction time.
    #[serde(default)]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub max_latency_ms: Option<u64>,

    // ── local-onnx ────────────────────────────────────────────────
    #[serde(default)]
    pub model_path: Option<std::path::PathBuf>,

    // ── shared ────────────────────────────────────────────────────
    #[serde(default)]
    pub require_confirmation: Option<bool>,
    #[serde(default)]
    pub weight: Option<f32>,
}
