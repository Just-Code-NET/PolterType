//! `RemoteLlmDetector` — opt-in remote HTTP detection.

use super::*;
use crate::AiError;
use poltertype_detect::{DetectionContext, Detector, Verdict};
#[cfg(feature = "remote")]
use tracing::info;
use tracing::warn;

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

    fn judge(&self, _ctx: &DetectionContext<'_>) -> Verdict {
        if !self.allow_remote {
            warn!(
                id = %self.id,
                "remote LLM detector skipped: allow_remote = false"
            );
            return Verdict::NoOpinion;
        }
        #[cfg(not(feature = "remote"))]
        {
            warn!(
                id = %self.id,
                "remote LLM detector skipped: built without `remote` feature"
            );
            Verdict::NoOpinion
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
            Verdict::NoOpinion
        }
    }
}
