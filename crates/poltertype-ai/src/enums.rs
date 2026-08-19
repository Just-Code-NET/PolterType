//! AI subsystem errors and the choices a plug-in entry switches on.

pub use poltertype_detect::{Detector, RewriteRequest, RewriteVerdict, Verdict, WordRewriter};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("keyring lookup for {0:?} failed: {1}")]
    KeyringLookup(String, String),
    #[cfg(feature = "remote")]
    #[error("LLM call failed: {0}")]
    Remote(#[from] reqwest::Error),
    #[error("LLM disabled: {0}")]
    RemoteDisabled(String),
    /// The `[[ai.plugins]]` entry does not describe a buildable
    /// plug-in. Always names the entry's `id`, so a user with three
    /// plug-ins learns which one is wrong.
    #[error("invalid plug-in config: {0}")]
    Config(String),
}

/// The wire shape of the endpoint we are talking to. Three cover
/// essentially everything: what is not Anthropic or Ollama's native
/// API has settled on OpenAI's chat-completions shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// `POST {"model", "messages":[…]}` → `choices[0].message.content`.
    /// llama.cpp, LM Studio, vLLM, OpenRouter, OpenAI itself, and
    /// Ollama's `/v1` compatibility layer all speak this.
    OpenAiChat,
    /// `POST {"model", "max_tokens", "messages":[…]}` →
    /// `content[0].text`, with `x-api-key` + `anthropic-version`.
    AnthropicMessages,
    /// Ollama's native `POST /api/generate` → `response`.
    OllamaGenerate,
}

impl WireFormat {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "openai-chat" | "openai" => Self::OpenAiChat,
            "anthropic-messages" | "anthropic" => Self::AnthropicMessages,
            "ollama-generate" | "ollama" => Self::OllamaGenerate,
            _ => return None,
        })
    }
}

/// When the query happens relative to the correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// Answer from cache; a miss returns no opinion and queues the
    /// query so the *next* occurrence is decided. The default, because
    /// it cannot slow a correction down.
    Background,
    /// Call inline, inside the deadline. Only sane against a local
    /// endpoint, and even then it is the user choosing to put a model
    /// in the path of their own typing.
    Blocking,
}

impl QueryMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "background" | "async" => Self::Background,
            "blocking" | "sync" => Self::Blocking,
            _ => return None,
        })
    }
}

/// Whether an endpoint's host keeps the request on this machine — the
/// distinction that lets a local model work without `allow_remote`.
/// See [`crate::locality`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locality {
    Loopback,
    Remote,
}
