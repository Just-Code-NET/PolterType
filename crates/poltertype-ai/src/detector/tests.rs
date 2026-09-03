use super::*;
use crate::enums::WireFormat;
use poltertype_types::LayoutId;

fn settings(endpoint: &str, allow_remote: bool, mode: QueryMode) -> LlmSettings {
    let locality = crate::locality::classify(endpoint);
    LlmSettings {
        id: "t".into(),
        endpoint: endpoint.into(),
        format: WireFormat::OpenAiChat,
        model: "m".into(),
        api_key: None,
        key_unavailable: false,
        max_latency_ms: 100,
        mode,
        cache_size: 16,
        locality,
        allow_remote,
    }
}

/// A loopback endpoint is *not* a network call in the sense
/// `allow_remote` exists to gate, so it must work without it.
#[test]
fn loopback_is_permitted_without_allow_remote() {
    let s = settings(
        "http://127.0.0.1:11434/api/generate",
        false,
        QueryMode::Background,
    );
    assert_eq!(s.locality, Locality::Loopback);
    assert_eq!(
        s.permitted(),
        cfg!(feature = "remote"),
        "only the cargo feature should gate a local endpoint"
    );
}

/// A remote endpoint must stay silent until the user says otherwise.
#[test]
fn remote_needs_allow_remote() {
    let denied = settings(
        "https://api.openai.com/v1/chat/completions",
        false,
        QueryMode::Background,
    );
    assert_eq!(denied.locality, Locality::Remote);
    assert!(
        !denied.permitted(),
        "remote without the switch must not run"
    );

    let allowed = settings(
        "https://api.openai.com/v1/chat/completions",
        true,
        QueryMode::Background,
    );
    assert_eq!(allowed.permitted(), cfg!(feature = "remote"));
}

/// The whole crate must be inert in a build without the HTTP client,
/// whatever the config says.
#[test]
#[cfg(not(feature = "remote"))]
fn without_the_cargo_feature_nothing_is_permitted() {
    for (ep, allow) in [
        ("http://127.0.0.1:11434/api/generate", true),
        ("https://api.openai.com/v1/chat/completions", true),
    ] {
        assert!(!settings(ep, allow, QueryMode::Background).permitted());
    }
}

fn ctx_layouts() -> (Vec<(LayoutId, String)>, LayoutId) {
    (
        vec![
            (LayoutId::from("en-US"), "ghbdsn".to_string()),
            (LayoutId::from("uk-UA"), "привіт".to_string()),
        ],
        LayoutId::from("en-US"),
    )
}

#[test]
fn a_chosen_other_layout_becomes_a_switch() {
    let (cands, current) = ctx_layouts();
    let ctx = DetectionContext {
        current_layout: &current,
        candidates: &cands,
        recent_context: "",
    };
    match to_verdict(Some(1), &ctx, "t") {
        Verdict::Switch(v) => assert_eq!(v.best_layout.as_str(), "uk-UA"),
        other => panic!("expected a switch, got {other:?}"),
    }
}

/// Picking the layout the user is already in is a vote to leave the
/// word alone — not a switch to where they already are.
#[test]
fn choosing_the_current_layout_is_a_keep() {
    let (cands, current) = ctx_layouts();
    let ctx = DetectionContext {
        current_layout: &current,
        candidates: &cands,
        recent_context: "",
    };
    assert!(matches!(
        to_verdict(Some(0), &ctx, "t"),
        Verdict::Keep { .. }
    ));
}

/// "None of these" and an index that no longer exists both have to
/// come out as no opinion rather than a wrong correction.
#[test]
fn undecidable_answers_are_no_opinion() {
    let (cands, current) = ctx_layouts();
    let ctx = DetectionContext {
        current_layout: &current,
        candidates: &cands,
        recent_context: "",
    };
    assert!(matches!(to_verdict(None, &ctx, "t"), Verdict::NoOpinion));
    assert!(
        matches!(to_verdict(Some(9), &ctx, "t"), Verdict::NoOpinion),
        "a stale index must not panic or mis-target"
    );
}

/// Background mode is the default and must never wait on a miss. This
/// asserts the property the whole cache design exists for: a cold
/// cache costs one no-opinion, not a round-trip.
#[test]
fn background_mode_returns_immediately_on_a_cache_miss() {
    let d = LlmDetector::new(settings(
        // Deliberately a port nothing listens on: if this ever blocked
        // it would block for the full timeout and the test would
        // notice by taking 100 ms+.
        "http://127.0.0.1:9/v1/chat/completions",
        false,
        QueryMode::Background,
    ))
    .expect("construct");

    let (cands, current) = ctx_layouts();
    let ctx = DetectionContext {
        current_layout: &current,
        candidates: &cands,
        recent_context: "",
    };
    let started = std::time::Instant::now();
    let verdict = d.judge(&ctx);
    let elapsed = started.elapsed();

    assert!(
        matches!(verdict, Verdict::NoOpinion),
        "cold cache: no opinion"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "background judge must not wait on the network; took {elapsed:?}"
    );
}

/// One candidate means there is nothing to choose between, so the
/// detector should not spend a query on it.
#[test]
fn a_single_candidate_is_never_queried() {
    let d = LlmDetector::new(settings(
        "http://127.0.0.1:9/v1/chat/completions",
        false,
        QueryMode::Background,
    ))
    .expect("construct");
    let cands = vec![(LayoutId::from("en-US"), "hello".to_string())];
    let current = LayoutId::from("en-US");
    let ctx = DetectionContext {
        current_layout: &current,
        candidates: &cands,
        recent_context: "",
    };
    assert!(matches!(d.judge(&ctx), Verdict::NoOpinion));
}

/// A cached answer is used without any call, in either mode.
///
/// Feature-gated because without an HTTP client the detector is inert
/// by design and returns before it ever consults the cache — which is
/// the behaviour `without_the_cargo_feature_nothing_is_permitted`
/// pins down, and there would be no way to populate the cache anyway.
#[test]
#[cfg(feature = "remote")]
fn a_cached_answer_is_served_from_memory() {
    let d = LlmDetector::new(settings(
        "http://127.0.0.1:9/v1/chat/completions",
        false,
        QueryMode::Blocking,
    ))
    .expect("construct");

    let (cands, current) = ctx_layouts();
    let texts: Vec<String> = cands.iter().map(|(_, t)| t.clone()).collect();
    d.remember(DecisionCache::key(&texts), Some(1));

    let ctx = DetectionContext {
        current_layout: &current,
        candidates: &cands,
        recent_context: "",
    };
    let started = std::time::Instant::now();
    let verdict = d.judge(&ctx);
    assert!(
        started.elapsed() < std::time::Duration::from_millis(50),
        "a cache hit must not touch the network even in blocking mode"
    );
    match verdict {
        Verdict::Switch(v) => assert_eq!(v.best_layout.as_str(), "uk-UA"),
        other => panic!("expected the cached switch, got {other:?}"),
    }
}
