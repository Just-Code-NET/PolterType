//! Declaration of one AI plug-in, as it appears in `config.toml`.
//!
//! Here rather than in `poltertype-ai` because two crates that cannot
//! see each other both need it: `poltertype-core` parses it as part of
//! the settings file, and `poltertype-ai` turns it into a detector.
//! Keeping the schema out of the optional crate also means a build
//! *without* the `ai` feature still parses an `[[ai.plugins]]` entry
//! instead of rejecting the user's config.

use serde::{Deserialize, Serialize};

/// One `[[ai.plugins]]` entry.
///
/// Deliberately one flat struct rather than a tagged enum: `type` is a
/// plain string so that a config naming a plug-in kind this build does
/// not know is *reported and skipped*, not a parse error that takes
/// the whole settings file down with it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AiPluginConfig {
    /// Which backend to construct. Today there is exactly one: `llm`.
    ///
    /// PolterType ships the *interface*, never a model or a bundled
    /// vendor client. Whatever answers at [`endpoint`](Self::endpoint)
    /// is the user's own choice — an Ollama on their machine, an API
    /// they hold the key to, a gateway of their own. We provide the
    /// socket; they decide what is plugged into it, if anything.
    pub r#type: String,
    /// Stable identifier, used in logs and to tell two entries of the
    /// same kind apart.
    pub id: String,

    /// Convenience preset that fills in `endpoint` and `format`:
    /// `ollama`, `openai`, `anthropic`, `llama-cpp`, `lm-studio`.
    /// Purely a shorthand — anything it sets, the explicit fields
    /// below override, and an entry may skip it entirely.
    #[serde(default)]
    pub provider: Option<String>,
    /// Full URL of the chat/completion endpoint to POST to.
    ///
    /// **A loopback host (`127.0.0.1`, `::1`, `localhost`) is treated
    /// as local** and works without `[ai].allow_remote`, because
    /// nothing leaves the machine. Any other host needs that switch
    /// turned on explicitly.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Request/response shape: `openai-chat`, `anthropic-messages`,
    /// or `ollama-generate`. Most self-hosted servers speak
    /// `openai-chat`.
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// `keyring:<entry>` — never the key itself. A literal secret here
    /// is rejected at construction time. Optional: a local Ollama
    /// needs no key at all.
    #[serde(default)]
    pub api_key_ref: Option<String>,
    /// How long a single query may take before it is abandoned.
    #[serde(default)]
    pub max_latency_ms: Option<u64>,
    /// `background` (default) or `blocking`.
    ///
    /// `judge` runs on the correction path, so `blocking` puts the
    /// round-trip between the user finishing a word and the word being
    /// fixed. `background` answers instantly from a cache and queries
    /// off-thread for next time, which costs the first occurrence of
    /// each word and nothing after it.
    #[serde(default)]
    pub mode: Option<String>,
    /// How many decided words to remember. `0` disables the cache,
    /// which in `background` mode means the detector never answers.
    #[serde(default)]
    pub cache_size: Option<usize>,

    // ── shared ────────────────────────────────────────────────────
    #[serde(default)]
    pub require_confirmation: Option<bool>,
    #[serde(default)]
    pub weight: Option<f32>,
}
