//! Configuration for AI plug-ins.

use serde::{Deserialize, Serialize};

/// Per-detector / per-rewriter configuration as it appears in
/// `config.toml`. The poltertype-ai subsystem will iterate this list and
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
