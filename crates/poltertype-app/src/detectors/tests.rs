//! Tests for the AI plug-in gate.
//!
//! What is being pinned here is the *gating*, not the detectors: that a
//! disabled subsystem builds nothing, that an enabled one with no
//! entries builds nothing, and that plug-ins are appended rather than
//! substituted. Those are the properties that decide whether turning
//! `[ai].enabled` on can hurt a user who was happy before.

use poltertype_core::settings::AiSettings;
use poltertype_types::AiPluginConfig;

use super::build_ai_detectors;

/// An entry pointing at a third-party API the user holds a key for.
fn remote_entry(id: &str) -> AiPluginConfig {
    AiPluginConfig {
        r#type: "llm".to_owned(),
        id: id.to_owned(),
        provider: Some("anthropic".to_owned()),
        model: Some("claude-sonnet-4".to_owned()),
        api_key_ref: Some("keyring:anthropic".to_owned()),
        ..Default::default()
    }
}

/// An entry pointing at a model on the user's own machine. Needs no
/// key and no `allow_remote`, because nothing leaves the computer.
fn local_entry(id: &str) -> AiPluginConfig {
    AiPluginConfig {
        r#type: "llm".to_owned(),
        id: id.to_owned(),
        provider: Some("ollama".to_owned()),
        model: Some("llama3".to_owned()),
        ..Default::default()
    }
}

/// The default. Nothing is built, and nothing is said.
#[test]
fn a_disabled_subsystem_builds_nothing() {
    let ai = AiSettings {
        enabled: false,
        allow_remote: false,
        plugins: vec![remote_entry("claude")],
    };
    assert!(
        build_ai_detectors(&ai).is_empty(),
        "plug-ins must not load while [ai].enabled is false"
    );
}

#[test]
fn enabled_with_no_plugins_builds_nothing() {
    let ai = AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: Vec::new(),
    };
    assert!(build_ai_detectors(&ai).is_empty());
}

/// With the `ai` feature the entry becomes a detector; without it the
/// crate is not linked and nothing can be built. Both are correct — the
/// test asserts the one this build is compiled for, so it is honest
/// either way rather than silently vacuous.
#[test]
fn an_enabled_entry_loads_only_where_the_feature_exists() {
    let ai = AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: vec![remote_entry("claude")],
    };
    let built = build_ai_detectors(&ai);
    if cfg!(feature = "ai") {
        assert_eq!(built.len(), 1, "the entry should have produced a detector");
    } else {
        assert!(
            built.is_empty(),
            "a build without the ai feature must produce nothing"
        );
    }
}

/// `allow_remote` gates the *call*, not the construction — so that
/// switching it on is a settings change rather than a config rewrite.
#[test]
fn allow_remote_does_not_decide_whether_the_plugin_loads() {
    let plugins = vec![remote_entry("claude")];
    let off = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: plugins.clone(),
    });
    let on = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: true,
        plugins,
    });
    assert_eq!(off.len(), on.len());
}

/// A model the user runs themselves loads with `allow_remote` off.
/// That switch exists to gate typed words *leaving the machine*, and
/// a request to loopback does not — requiring it here would make
/// people enable network access they are not using.
#[test]
fn a_local_model_needs_no_network_permission() {
    let built = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: vec![local_entry("ollama")],
    });
    if cfg!(feature = "ai") {
        assert_eq!(built.len(), 1, "a loopback endpoint must load");
    } else {
        assert!(built.is_empty());
    }
}

/// A config written for 0.9.0 names plug-in kinds that no longer
/// exist. It must cost the user a log line, not the app: the entry is
/// skipped and anything valid beside it still loads.
#[test]
fn a_retired_plugin_kind_is_skipped_without_taking_the_rest_down() {
    let mut old = local_entry("stale");
    old.r#type = "local-onnx".to_owned();

    let built = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: vec![old, local_entry("current")],
    });
    if cfg!(feature = "ai") {
        assert_eq!(
            built.len(),
            1,
            "the retired entry goes, the valid one stays"
        );
    } else {
        assert!(built.is_empty());
    }
}
