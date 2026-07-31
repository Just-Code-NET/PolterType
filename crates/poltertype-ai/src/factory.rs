//! Turn `[[ai.plugins]]` entries into detectors.
//!
//! This is the seam that was missing: the crate has had backends since
//! v0.1 and nothing ever constructed them, so `[ai].enabled` was a
//! setting no code read. Everything below is about being *safe to
//! enable*, because the pipeline this feeds runs on the correction
//! path.
//!
//! Three rules the whole module exists to enforce:
//!
//! * **One bad entry never costs the others.** A plug-in that cannot be
//!   built is logged with its id and skipped; the rest still load. A
//!   config typo must not silently disable the AI subsystem entirely,
//!   and must never take down the engine.
//! * **A secret in `config.toml` is refused, not used.** `api_key_ref`
//!   has to be a `keyring:` reference. Accepting a literal key would
//!   quietly teach users to put one in a plain-text file that they
//!   might well paste into a bug report.
//! * **Remote stays behind both switches.** The cargo feature decides
//!   whether the code exists; `[ai].allow_remote` decides whether it
//!   may run. A detector built with `allow_remote = false` returns no
//!   opinion rather than failing to construct — so flipping the
//!   setting takes effect on the next restart without editing config.

use poltertype_detect::Detector;
use poltertype_types::AiPluginConfig;
use tracing::{info, warn};

use crate::AiError;
use crate::local::LocalOnnxDetector;
use crate::remote::{Provider, RemoteLlmDetector};

/// Plug-in kind strings accepted in `type`.
pub const TYPE_LOCAL_ONNX: &str = "local-onnx";
pub const TYPE_REMOTE_LLM: &str = "remote-llm";

/// Default budget for a remote call, if the entry does not set one.
const DEFAULT_MAX_LATENCY_MS: u64 = 400;

/// Build every detector the config asks for, skipping the ones that
/// cannot be built.
///
/// Returns them in configuration order. The caller appends these after
/// the built-in detectors, so a plug-in adds a voice to the decision
/// rather than replacing the ones that work offline.
pub fn build_detectors(plugins: &[AiPluginConfig], allow_remote: bool) -> Vec<Box<dyn Detector>> {
    let mut out: Vec<Box<dyn Detector>> = Vec::new();
    for cfg in plugins {
        match build_one(cfg, allow_remote) {
            Ok(d) => {
                info!(id = %cfg.id, kind = %cfg.r#type, "AI plug-in loaded");
                out.push(d);
            }
            Err(e) => warn!(
                id = %cfg.id,
                kind = %cfg.r#type,
                %e,
                "AI plug-in skipped; the other detectors are unaffected"
            ),
        }
    }
    out
}

fn build_one(cfg: &AiPluginConfig, allow_remote: bool) -> Result<Box<dyn Detector>, AiError> {
    match cfg.r#type.as_str() {
        TYPE_LOCAL_ONNX => {
            let path = cfg
                .model_path
                .clone()
                .ok_or_else(|| AiError::Config("local-onnx needs `model_path`".into()))?;
            Ok(Box::new(LocalOnnxDetector::new(cfg.id.clone(), path)?))
        }
        TYPE_REMOTE_LLM => {
            let provider_name = cfg
                .provider
                .as_deref()
                .ok_or_else(|| AiError::Config("remote-llm needs `provider`".into()))?;
            let provider = Provider::parse(provider_name)
                .ok_or_else(|| AiError::Config(format!("unknown provider `{provider_name}`")))?;
            let model = cfg
                .model
                .clone()
                .ok_or_else(|| AiError::Config("remote-llm needs `model`".into()))?;
            let api_key_ref = cfg
                .api_key_ref
                .clone()
                .ok_or_else(|| AiError::Config("remote-llm needs `api_key_ref`".into()))?;
            // The one validation worth failing construction over: a key
            // pasted into config.toml is a key in the user's backups,
            // their dotfiles repo, and any log they attach to an issue.
            if !api_key_ref.starts_with("keyring:") {
                return Err(AiError::Config(
                    "`api_key_ref` must be a `keyring:<entry>` reference — never the key itself"
                        .into(),
                ));
            }
            Ok(Box::new(RemoteLlmDetector::new(
                cfg.id.clone(),
                provider,
                model,
                api_key_ref,
                cfg.max_latency_ms.unwrap_or(DEFAULT_MAX_LATENCY_MS),
                allow_remote,
            )?))
        }
        other => Err(AiError::Config(format!(
            "unknown plug-in type `{other}` (expected `{TYPE_LOCAL_ONNX}` or `{TYPE_REMOTE_LLM}`)"
        ))),
    }
}

#[cfg(test)]
mod tests;
