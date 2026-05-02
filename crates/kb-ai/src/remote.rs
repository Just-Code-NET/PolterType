//! Remote LLM-backed language detector.
//!
//! Gated behind the `remote` cargo feature *and* the
//! `[ai].allow_remote` runtime setting. Even with both on, the
//! detector only fires when the existing pipeline reports low
//! confidence (configurable per-detector). All calls are subject to
//! a `max_latency_ms` budget — anything slower is dropped, since the
//! engine should never block typing.

use kb_detect::{DetectionContext, DetectionVerdict, Detector};
#[cfg(feature = "remote")]
use tracing::info;
use tracing::warn;

use crate::AiError;

pub struct RemoteLlmDetector {
    pub id: String,
    pub provider: Provider,
    pub model: String,
    pub api_key_ref: String,
    pub max_latency_ms: u64,
    pub allow_remote: bool,
    #[cfg(feature = "remote")]
    client: reqwest::blocking::Client,
}

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Ollama,
    Custom,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "anthropic" => Self::Anthropic,
            "openai" => Self::OpenAi,
            "ollama" => Self::Ollama,
            "custom-openai-compatible" | "custom" => Self::Custom,
            _ => return None,
        })
    }
}

impl RemoteLlmDetector {
    #[cfg(feature = "remote")]
    pub fn new(
        id: String,
        provider: Provider,
        model: String,
        api_key_ref: String,
        max_latency_ms: u64,
        allow_remote: bool,
    ) -> Result<Self, AiError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(max_latency_ms.max(100)))
            .build()
            .map_err(AiError::Remote)?;
        Ok(Self {
            id,
            provider,
            model,
            api_key_ref,
            max_latency_ms,
            allow_remote,
            client,
        })
    }

    #[cfg(not(feature = "remote"))]
    pub fn new(
        id: String,
        provider: Provider,
        model: String,
        api_key_ref: String,
        max_latency_ms: u64,
        allow_remote: bool,
    ) -> Result<Self, AiError> {
        Ok(Self {
            id,
            provider,
            model,
            api_key_ref,
            max_latency_ms,
            allow_remote,
        })
    }
}

impl Detector for RemoteLlmDetector {
    fn name(&self) -> &'static str {
        "remote-llm"
    }

    fn detect(&self, _ctx: &DetectionContext<'_>) -> Option<DetectionVerdict> {
        if !self.allow_remote {
            warn!(
                id = %self.id,
                "remote LLM detector skipped: allow_remote = false"
            );
            return None;
        }
        #[cfg(not(feature = "remote"))]
        {
            warn!(
                id = %self.id,
                "remote LLM detector skipped: built without `remote` feature"
            );
            None
        }
        // Real call goes here in v0.1.x. We keep the function honest
        // about its current state rather than ship a half-baked
        // detector that misbehaves under real load.
        #[cfg(feature = "remote")]
        {
            info!(
                id = %self.id,
                provider = ?self.provider,
                model = %self.model,
                "remote LLM detector would call out (stub)"
            );
            let _ = &self.client; // silence unused
            let _ = &self.api_key_ref;
            None
        }
    }
}
