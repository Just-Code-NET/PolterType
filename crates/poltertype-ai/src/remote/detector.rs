//! `RemoteLlmDetector` — opt-in remote HTTP detection.

use super::*;
use crate::AiError;
use poltertype_detect::{DetectionContext, Detector, Verdict};
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
        let built = Self {
            id,
            provider,
            model,
            api_key_ref,
            max_latency_ms,
            allow_remote,
            client,
        };
        built.announce();
        Ok(built)
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
        let built = Self {
            id,
            provider,
            model,
            api_key_ref,
            max_latency_ms,
            allow_remote,
        };
        built.announce();
        Ok(built)
    }

    /// Say once, at construction, why this detector will or will not
    /// have an opinion. `judge` stays silent — it runs per word.
    fn announce(&self) {
        if !cfg!(feature = "remote") {
            warn!(
                id = %self.id,
                "remote LLM detector loaded but built without the `remote` cargo feature — \
                 it will return no opinion"
            );
        } else if !self.allow_remote {
            warn!(
                id = %self.id,
                "remote LLM detector loaded but `[ai].allow_remote = false` — it will return \
                 no opinion until that is switched on"
            );
        } else {
            warn!(
                id = %self.id,
                provider = ?self.provider,
                model = %self.model,
                "remote LLM detector is a stub: it makes no request and returns no opinion. \
                 No network call is performed."
            );
        }
    }
}

impl Detector for RemoteLlmDetector {
    fn name(&self) -> &'static str {
        "remote-llm"
    }

    fn judge(&self, _ctx: &DetectionContext<'_>) -> Verdict {
        // Every early return here is silent on purpose: this runs per
        // word boundary, and a detector that logs on the correction
        // path is a detector that costs more than it gives. The
        // reasons are reported once, at construction.
        if !self.allow_remote {
            return Verdict::NoOpinion;
        }
        #[cfg(not(feature = "remote"))]
        {
            Verdict::NoOpinion
        }
        // Real call goes here in v0.1.x. We keep the function honest
        // about its current state rather than ship a half-baked
        // detector that misbehaves under real load.
        #[cfg(feature = "remote")]
        {
            let _ = &self.client;
            let _ = &self.api_key_ref;
            Verdict::NoOpinion
        }
    }
}
