//! Tests for plug-in construction.
//!
//! The theme is that a wrong config file is a normal thing to have and
//! must never be dangerous: it should cost the user one warning line
//! naming the entry, not a disabled subsystem and certainly not a
//! crash on the correction path.

use super::*;

fn cfg(kind: &str, id: &str) -> AiPluginConfig {
    AiPluginConfig {
        r#type: kind.to_owned(),
        id: id.to_owned(),
        ..Default::default()
    }
}

#[test]
fn an_empty_config_builds_nothing() {
    assert!(build_detectors(&[], false).is_empty());
}

#[test]
fn an_unknown_plugin_type_is_skipped_not_fatal() {
    let plugins = vec![cfg("quantum-oracle", "weird")];
    assert!(build_detectors(&plugins, false).is_empty());
}

/// The property that matters most: one broken entry must not cost the
/// others. A user with three plug-ins and one typo should still get
/// the working ones.
#[test]
fn a_broken_entry_does_not_take_its_neighbours_down() {
    let mut good = cfg(TYPE_REMOTE_LLM, "ok");
    good.provider = Some("anthropic".into());
    good.model = Some("claude-sonnet-4".into());
    good.api_key_ref = Some("keyring:anthropic".into());

    let plugins = vec![cfg("nonsense", "bad"), good];
    assert_eq!(
        build_detectors(&plugins, false).len(),
        1,
        "the valid entry must still load"
    );
}

/// A key in `config.toml` ends up in backups, dotfile repos and pasted
/// bug reports. Refusing to construct is the only answer that does not
/// teach the habit.
#[test]
fn a_literal_api_key_is_refused() {
    let mut c = cfg(TYPE_REMOTE_LLM, "leaky");
    c.provider = Some("anthropic".into());
    c.model = Some("claude-sonnet-4".into());
    c.api_key_ref = Some("sk-ant-totally-a-real-key".into());

    match build_one(&c, true) {
        Err(AiError::Config(m)) => assert!(m.contains("keyring:"), "{m}"),
        Err(e) => panic!("wrong error for a literal key: {e}"),
        Ok(_) => panic!("a literal key must not build"),
    }
}

#[test]
fn remote_needs_provider_model_and_key_reference() {
    for missing in ["provider", "model", "api_key_ref"] {
        let mut c = cfg(TYPE_REMOTE_LLM, "partial");
        if missing != "provider" {
            c.provider = Some("openai".into());
        }
        if missing != "model" {
            c.model = Some("gpt-4o-mini".into());
        }
        if missing != "api_key_ref" {
            c.api_key_ref = Some("keyring:openai".into());
        }
        assert!(
            build_one(&c, true).is_err(),
            "a remote plug-in without `{missing}` must not build"
        );
    }
}

#[test]
fn an_unknown_provider_is_refused() {
    let mut c = cfg(TYPE_REMOTE_LLM, "who");
    c.provider = Some("skynet".into());
    c.model = Some("t1000".into());
    c.api_key_ref = Some("keyring:skynet".into());
    assert!(build_one(&c, true).is_err());
}

/// `allow_remote = false` must still *build* the detector — the switch
/// is consulted per judgement, so flipping the setting takes effect on
/// restart without the user editing their plug-in entry.
#[test]
fn allow_remote_false_still_builds_and_simply_holds_its_tongue() {
    let mut c = cfg(TYPE_REMOTE_LLM, "quiet");
    c.provider = Some("ollama".into());
    c.model = Some("llama3".into());
    c.api_key_ref = Some("keyring:ollama".into());
    assert!(build_one(&c, false).is_ok());
}

#[test]
fn local_onnx_needs_a_model_path() {
    assert!(build_one(&cfg(TYPE_LOCAL_ONNX, "nopath"), false).is_err());
}

#[test]
fn local_onnx_refuses_a_model_that_is_not_there() {
    let mut c = cfg(TYPE_LOCAL_ONNX, "ghost");
    c.model_path = Some("/nonexistent/definitely-not-here.onnx".into());
    match build_one(&c, false) {
        Err(AiError::ModelMissing(_)) => {}
        Err(e) => panic!("wrong error for a missing model: {e}"),
        Ok(_) => panic!("a missing model must not build"),
    }
}
