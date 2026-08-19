//! Turn `[[ai.plugins]]` entries into detectors.
//!
//! Everything here is about being *safe to enable*, because the
//! pipeline this feeds runs on the correction path. Four rules:
//!
//! * **One bad entry never costs the others** — it is logged with its
//!   id and skipped, so a config typo cannot silently disable the whole
//!   subsystem or take down the engine.
//! * **A secret in `config.toml` is refused, not used.** `api_key_ref`
//!   must be a `keyring:` reference; accepting a literal key would
//!   teach users to keep one in a file they might paste into a bug
//!   report.
//! * **A detector that may not run** (see [`crate`] for the gates)
//!   returns no opinion rather than failing to construct.
//! * **A blocking entry cannot be configured into ruining typing.** The
//!   deadline is capped here, at build time, where the user is told —
//!   not clamped silently later where they would just feel the lag.

use poltertype_detect::Detector;
use poltertype_types::AiPluginConfig;
use tracing::{info, warn};

use crate::AiError;
use crate::consts::{
    DEFAULT_CACHE_SIZE, DEFAULT_MAX_LATENCY_MS, MAX_BLOCKING_LATENCY_MS, PRESETS, RETIRED_TYPES,
    TYPE_LLM,
};
use crate::detector::{LlmDetector, LlmSettings};
use crate::enums::{Locality, QueryMode, WireFormat};
use crate::keys::resolve_api_key;
use crate::locality;

/// Build every detector the config asks for, skipping the ones that
/// cannot be built, in configuration order. The caller appends these
/// after the built-in detectors, so a plug-in adds a voice to the
/// decision rather than replacing the ones that work offline.
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
    if RETIRED_TYPES.contains(&cfg.r#type.as_str()) {
        return Err(AiError::Config(format!(
            "plug-in type `{}` was removed in 0.10.0. PolterType no longer ships a bundled model \
             or a vendor-specific client — use `type = \"{TYPE_LLM}\"` and point `endpoint` at a \
             model you run or an API you hold the key to. See docs/AI.md.",
            cfg.r#type
        )));
    }
    if cfg.r#type != TYPE_LLM {
        return Err(AiError::Config(format!(
            "unknown plug-in type `{}` (expected `{TYPE_LLM}`)",
            cfg.r#type
        )));
    }

    let (endpoint, format) = resolve_endpoint(cfg)?;
    let model = cfg
        .model
        .clone()
        .ok_or_else(|| AiError::Config("`model` is required — name the model to ask".into()))?;
    let mode = match cfg.mode.as_deref() {
        None => QueryMode::Background,
        Some(s) => QueryMode::parse(s).ok_or_else(|| {
            AiError::Config(format!(
                "unknown mode `{s}` (expected `background` or `blocking`)"
            ))
        })?,
    };
    let max_latency_ms = resolve_latency(cfg, mode)?;
    let locality = locality::classify(&endpoint);
    let (api_key, key_unavailable) = resolve_key(cfg, locality)?;

    Ok(Box::new(LlmDetector::new(LlmSettings {
        id: cfg.id.clone(),
        endpoint,
        format,
        model,
        api_key,
        key_unavailable,
        max_latency_ms,
        mode,
        cache_size: cfg.cache_size.unwrap_or(DEFAULT_CACHE_SIZE),
        locality,
        allow_remote,
    })?))
}

/// Work out where to send the request and what shape it takes.
///
/// A `provider` preset fills in whatever the entry left blank; explicit
/// fields always win. An entry with neither is an error rather than a
/// default — any endpoint we picked would be choosing a vendor on the
/// user's behalf.
fn resolve_endpoint(cfg: &AiPluginConfig) -> Result<(String, WireFormat), AiError> {
    let preset = match cfg.provider.as_deref() {
        None => None,
        Some(name) => Some(PRESETS.iter().find(|(p, _, _)| *p == name).ok_or_else(|| {
            let known: Vec<&str> = PRESETS.iter().map(|(p, _, _)| *p).collect();
            AiError::Config(format!(
                "unknown provider `{name}` (known presets: {}). `provider` is only a \
                         shorthand — set `endpoint` and `format` directly for anything else.",
                known.join(", ")
            ))
        })?),
    };

    let endpoint = cfg
        .endpoint
        .clone()
        .or_else(|| preset.map(|(_, url, _)| (*url).to_owned()))
        .ok_or_else(|| {
            AiError::Config(
                "needs an `endpoint` (or a `provider` preset to supply one). PolterType ships no \
                 default endpoint — what answers is your choice."
                    .into(),
            )
        })?;

    let format = match cfg.format.as_deref() {
        Some(s) => WireFormat::parse(s).ok_or_else(|| {
            AiError::Config(format!(
                "unknown format `{s}` (expected `openai-chat`, `anthropic-messages` or \
                 `ollama-generate`)"
            ))
        })?,
        None => preset.map(|(_, _, f)| *f).ok_or_else(|| {
            AiError::Config(
                "needs a `format` (or a `provider` preset to supply one). Most self-hosted \
                 servers speak `openai-chat`."
                    .into(),
            )
        })?,
    };

    Ok((endpoint, format))
}

fn resolve_latency(cfg: &AiPluginConfig, mode: QueryMode) -> Result<u64, AiError> {
    let requested = cfg.max_latency_ms.unwrap_or(DEFAULT_MAX_LATENCY_MS);
    if mode == QueryMode::Blocking && requested > MAX_BLOCKING_LATENCY_MS {
        return Err(AiError::Config(format!(
            "`mode = \"blocking\"` puts this call between the user finishing a word and the word \
             being corrected, so `max_latency_ms` may not exceed {MAX_BLOCKING_LATENCY_MS} \
             (got {requested}). Either lower it or use the default background mode, which never \
             waits."
        )));
    }
    Ok(requested)
}

/// Resolve the API key, if there is one to resolve.
///
/// Returns `(key, unavailable)`. A key is optional: a local Ollama
/// needs none, and a remote endpoint may authenticate by IP — though
/// that case gets a log line, since a forgotten setting is likelier.
///
/// A keychain that cannot answer is **not** a construction failure —
/// a locked keychain is often temporary, so the detector is built and
/// stays quiet.
fn resolve_key(
    cfg: &AiPluginConfig,
    locality: Locality,
) -> Result<(Option<String>, bool), AiError> {
    let Some(reference) = cfg.api_key_ref.as_deref() else {
        if locality == Locality::Remote {
            info!(
                id = %cfg.id,
                "no `api_key_ref` for a remote endpoint — sending unauthenticated"
            );
        }
        return Ok((None, false));
    };
    // The one validation worth failing construction over: a secret
    // pasted into config.toml is a secret in the user's backups, their
    // dotfiles repo, and any log they attach to an issue.
    if !reference.starts_with("keyring:") {
        return Err(AiError::Config(
            "`api_key_ref` must be a `keyring:<entry>` reference — never the key itself".into(),
        ));
    }
    match resolve_api_key(reference) {
        Ok(key) => Ok((Some(key), false)),
        Err(e) => {
            warn!(
                id = %cfg.id,
                %e,
                "AI plug-in configured with a key the keychain cannot supply — the plug-in \
                 loads but stays silent. Store the secret under that entry name and restart."
            );
            Ok((None, true))
        }
    }
}

#[cfg(test)]
mod tests;
