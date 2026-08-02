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

/// `Box<dyn Detector>` is not `Debug`, so `expect_err` cannot be used
/// directly on a `build_one` result. This says the same thing.
fn refusal(c: &AiPluginConfig) -> AiError {
    match build_one(c, false) {
        Ok(_) => panic!("entry should have been refused, but it built"),
        Err(e) => e,
    }
}

/// A minimal entry that should build: a local model, no key needed.
fn local_entry(id: &str) -> AiPluginConfig {
    let mut c = cfg(TYPE_LLM, id);
    c.provider = Some("ollama".into());
    c.model = Some("llama3".into());
    c
}

#[test]
fn an_empty_config_builds_nothing() {
    assert!(build_detectors(&[], false).is_empty());
}

#[test]
fn an_unknown_plugin_type_is_skipped_not_fatal() {
    assert!(build_detectors(&[cfg("quantum-oracle", "weird")], false).is_empty());
}

/// The property that matters most: one broken entry must not cost the
/// others. A user with two plug-ins and one typo should still get the
/// working one.
#[test]
fn a_broken_entry_does_not_take_its_neighbours_down() {
    let plugins = vec![cfg("nonsense", "bad"), local_entry("ok")];
    assert_eq!(build_detectors(&plugins, false).len(), 1);
}

/// The retired kinds get a message that says what to do instead,
/// rather than the generic "unknown type" — anyone hitting these
/// wrote their config against an older PolterType.
#[test]
fn retired_plugin_types_explain_themselves() {
    for kind in RETIRED_TYPES {
        let err = refusal(&cfg(kind, "old"));
        let msg = err.to_string();
        assert!(msg.contains("removed in 0.10.0"), "{kind}: {msg}");
        assert!(
            msg.contains(TYPE_LLM),
            "{kind} should point at the new type: {msg}"
        );
    }
}

// ── the endpoint is the user's choice ────────────────────────────────

/// There is no default endpoint on purpose: picking one would be
/// choosing a vendor for the user.
#[test]
fn an_entry_without_an_endpoint_or_preset_is_refused() {
    let mut c = cfg(TYPE_LLM, "x");
    c.model = Some("m".into());
    let err = refusal(&c);
    assert!(err.to_string().contains("endpoint"), "{err}");
}

#[test]
fn a_preset_supplies_endpoint_and_format() {
    let (endpoint, format) = resolve_endpoint(&local_entry("x")).expect("ollama preset");
    assert!(endpoint.contains("11434"), "ollama's port: {endpoint}");
    assert_eq!(format, WireFormat::OllamaGenerate);
}

/// A preset is only a shorthand. Anything stated explicitly wins, so a
/// user can point the `ollama` preset at a box on another port.
#[test]
fn explicit_fields_override_the_preset() {
    let mut c = local_entry("x");
    c.endpoint = Some("http://127.0.0.1:9999/v1/chat/completions".into());
    c.format = Some("openai-chat".into());
    let (endpoint, format) = resolve_endpoint(&c).expect("build");
    assert_eq!(endpoint, "http://127.0.0.1:9999/v1/chat/completions");
    assert_eq!(format, WireFormat::OpenAiChat);
}

#[test]
fn an_unknown_preset_lists_the_known_ones() {
    let mut c = cfg(TYPE_LLM, "x");
    c.provider = Some("hal9000".into());
    c.model = Some("m".into());
    let err = refusal(&c);
    let msg = err.to_string();
    assert!(msg.contains("hal9000"), "names the bad value: {msg}");
    assert!(msg.contains("ollama"), "lists a known preset: {msg}");
}

/// An endpoint with no preset needs its format stated — we will not
/// guess a wire shape and send the user's words in the wrong envelope.
#[test]
fn an_endpoint_without_a_format_is_refused() {
    let mut c = cfg(TYPE_LLM, "x");
    c.endpoint = Some("https://gateway.example.com/v1/chat".into());
    c.model = Some("m".into());
    let err = refusal(&c);
    assert!(err.to_string().contains("format"), "{err}");
}

#[test]
fn a_model_is_always_required() {
    let mut c = cfg(TYPE_LLM, "x");
    c.provider = Some("ollama".into());
    let err = refusal(&c);
    assert!(err.to_string().contains("model"), "{err}");
}

// ── secrets ──────────────────────────────────────────────────────────

/// A key pasted into config.toml ends up in backups, dotfile repos and
/// pasted bug reports. Refuse it rather than use it.
#[test]
fn a_literal_api_key_is_refused() {
    let mut c = local_entry("x");
    c.api_key_ref = Some("sk-ant-secret-value".into());
    let err = refusal(&c);
    assert!(err.to_string().contains("keyring:"), "{err}");
}

/// A local model needs no credential; demanding a placeholder would be
/// theatre.
#[test]
fn a_local_endpoint_needs_no_key() {
    let (key, unavailable) = resolve_key(&local_entry("x"), Locality::Loopback)
        .expect("an entry with no key at all is fine");
    assert!(key.is_none());
    assert!(!unavailable, "no key wanted is not the same as one missing");
}

/// A keychain that cannot answer is a runtime condition, not a broken
/// config: the entry still builds and simply stays quiet, the same way
/// `allow_remote = false` does. Failing construction here would report
/// a config problem for something config cannot fix.
#[test]
fn a_key_the_keychain_cannot_supply_does_not_fail_construction() {
    let mut c = local_entry("x");
    // An entry name nothing will have stored.
    c.api_key_ref = Some("keyring:poltertype-test-definitely-absent".into());

    let (key, unavailable) =
        resolve_key(&c, Locality::Remote).expect("a missing secret is not a config error");
    // On a host with no keychain service at all this is the same
    // outcome, which is the point: either way the plug-in is inert
    // rather than absent.
    assert!(key.is_none() || !unavailable);

    assert!(
        build_one(&c, true).is_ok(),
        "the plug-in must still load so the log can explain itself"
    );
}

// ── the blocking-mode guard ──────────────────────────────────────────

/// `blocking` puts the round-trip between the user finishing a word
/// and the word being fixed. The cap is enforced where the user is
/// told about it, not silently clamped later where they would just
/// experience it as lag.
#[test]
fn a_slow_blocking_entry_is_refused_with_an_explanation() {
    let mut c = local_entry("x");
    c.mode = Some("blocking".into());
    c.max_latency_ms = Some(5_000);
    let err = refusal(&c);
    let msg = err.to_string();
    assert!(msg.contains(&MAX_BLOCKING_LATENCY_MS.to_string()), "{msg}");
    assert!(
        msg.contains("background"),
        "should point at the way out: {msg}"
    );
}

#[test]
fn a_fast_blocking_entry_is_allowed() {
    let mut c = local_entry("x");
    c.mode = Some("blocking".into());
    c.max_latency_ms = Some(120);
    assert!(build_one(&c, false).is_ok());
}

/// The generous default only applies to background mode, where it
/// costs nobody anything.
#[test]
fn background_mode_keeps_the_generous_default() {
    let c = local_entry("x");
    assert_eq!(
        resolve_latency(&c, QueryMode::Background).expect("ok"),
        DEFAULT_MAX_LATENCY_MS
    );
}

#[test]
fn an_unknown_mode_is_refused() {
    let mut c = local_entry("x");
    c.mode = Some("eventually".into());
    let err = refusal(&c);
    assert!(err.to_string().contains("eventually"), "{err}");
}

// ── the gate that matters ────────────────────────────────────────────

/// A remote entry still *builds* without `allow_remote` — it just
/// returns no opinion — so that flipping the setting takes effect on
/// restart without the user editing their plug-in entry.
#[test]
fn a_remote_entry_builds_but_stays_quiet_without_permission() {
    let mut c = cfg(TYPE_LLM, "x");
    c.provider = Some("anthropic".into());
    c.model = Some("claude-sonnet-4".into());
    assert_eq!(
        build_detectors(std::slice::from_ref(&c), false).len(),
        1,
        "it should build so the setting alone controls it"
    );
}

/// A local entry is unaffected by `allow_remote` in either position —
/// this is the whole point of the loopback distinction.
#[test]
fn a_local_entry_builds_regardless_of_allow_remote() {
    let c = local_entry("x");
    for allow in [false, true] {
        assert_eq!(build_detectors(std::slice::from_ref(&c), allow).len(), 1);
    }
}
