use super::*;

fn question<'a>(model: &'a str, cands: &'a [String]) -> Question<'a> {
    Question {
        model,
        candidates: cands,
    }
}

#[test]
fn prompt_numbers_candidates_from_one() {
    let cands = vec!["привіт".to_string(), "ghbdsn".to_string()];
    let q = question("m", &cands);
    assert_eq!(q.prompt(), "1. привіт\n2. ghbdsn\n");
}

/// The prompt carries the candidate words and nothing else — no
/// layout ids (which would reveal the user's installed languages), no
/// surrounding sentence, no application name.
#[test]
fn prompt_leaks_nothing_but_the_candidates() {
    let cands = vec!["слово".to_string()];
    let q = question("gpt-4o-mini", &cands);
    let p = q.prompt();
    assert!(!p.contains("uk-UA"), "no layout ids: {p}");
    assert!(!p.contains("en-US"), "no layout ids: {p}");
    assert_eq!(p.lines().count(), 1, "one line per candidate: {p}");
}

#[test]
fn bodies_are_valid_json_for_every_format() {
    let cands = vec!["té\"st".to_string(), "with\\slash".to_string()];
    let q = question("some-model", &cands);
    for format in [
        WireFormat::OpenAiChat,
        WireFormat::AnthropicMessages,
        WireFormat::OllamaGenerate,
    ] {
        let body = request_body(format, &q);
        // Round-trip through a real parser to prove the hand-rolled
        // escaping is correct, including the embedded quote/backslash.
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
        assert!(parsed.is_ok(), "{format:?} produced invalid JSON: {body}");
        assert!(
            body.contains("some-model"),
            "{format:?} must name the model"
        );
    }
}

#[test]
fn openai_and_ollama_bodies_disable_streaming_and_cap_output() {
    let cands = vec!["a".to_string()];
    let q = question("m", &cands);
    let ollama = request_body(WireFormat::OllamaGenerate, &q);
    assert!(
        ollama.contains(r#""stream":false"#),
        "a streamed reply would break the single-shot parse: {ollama}"
    );
    let openai = request_body(WireFormat::OpenAiChat, &q);
    assert!(openai.contains(r#""max_tokens":4"#), "cap the reply");
}

#[test]
fn anthropic_gets_its_key_header_and_version() {
    let h = headers(WireFormat::AnthropicMessages, Some("sk-test"));
    assert!(h.iter().any(|(k, v)| k == "x-api-key" && v == "sk-test"));
    assert!(h.iter().any(|(k, _)| k == "anthropic-version"));
    assert!(
        !h.iter().any(|(k, _)| k == "authorization"),
        "anthropic does not use a bearer token"
    );
}

#[test]
fn openai_gets_a_bearer_and_no_key_means_no_header() {
    let h = headers(WireFormat::OpenAiChat, Some("sk-test"));
    assert!(
        h.iter()
            .any(|(k, v)| k == "authorization" && v == "Bearer sk-test")
    );
    assert!(
        headers(WireFormat::OpenAiChat, None).is_empty(),
        "a local Ollama needs no key and must not get an empty one"
    );
}

#[test]
fn extracts_the_answer_from_each_response_shape() {
    let cases = [
        (
            WireFormat::OpenAiChat,
            r#"{"choices":[{"message":{"role":"assistant","content":"2"}}]}"#,
            "2",
        ),
        (
            WireFormat::AnthropicMessages,
            r#"{"content":[{"type":"text","text":"1"}]}"#,
            "1",
        ),
        (
            WireFormat::OllamaGenerate,
            r#"{"model":"llama3","response":"3","done":true}"#,
            "3",
        ),
    ];
    for (format, body, want) in cases {
        assert_eq!(
            extract_text(format, body).as_deref(),
            Some(want),
            "{format:?} on {body}"
        );
    }
}

#[test]
fn extraction_handles_escapes_and_survives_junk() {
    assert_eq!(
        extract_text(WireFormat::OllamaGenerate, r#"{"response":"a\"b\nc"}"#).as_deref(),
        Some("a\"b\nc")
    );
    // An error payload, a truncated body, or an unexpected shape all
    // have to come back as None rather than panic.
    for body in [
        r#"{"error":{"message":"bad key"}}"#,
        r#"{"response":"unterminated"#,
        "",
        "not json at all",
    ] {
        let _ = extract_text(WireFormat::OllamaGenerate, body);
    }
}

#[test]
fn parses_the_choice_a_model_actually_returns() {
    assert_eq!(parse_choice("2", 3), Some(1));
    assert_eq!(parse_choice(" 1 ", 3), Some(0));
    assert_eq!(parse_choice("1.", 3), Some(0));
    assert_eq!(parse_choice("\"2\"", 3), Some(1));
    assert_eq!(parse_choice("The answer is 2", 3), Some(1));
}

/// Everything ambiguous is no opinion. This runs on the correction
/// path: a wrong confident answer retypes the user's word incorrectly,
/// whereas no answer just leaves the offline detectors in charge.
#[test]
fn anything_unusable_is_no_opinion() {
    for reply in ["0", "", "none", "4", "99", "-1", "I'm not sure"] {
        assert_eq!(parse_choice(reply, 3), None, "reply {reply:?}");
    }
}
