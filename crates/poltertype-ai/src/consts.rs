//! Fixed values the LLM plug-in is built around.

use crate::enums::WireFormat;

/// Plug-in kind accepted in `type`.
pub const TYPE_LLM: &str = "llm";

/// Kind strings from before 0.10.0, kept only to produce a useful
/// error. They named backends PolterType no longer provides: a bundled
/// ONNX model and a vendor-specific client.
pub const RETIRED_TYPES: &[&str] = &["local-onnx", "remote-llm"];

/// Budget for one query when the entry does not set one. Generous
/// because the default mode is off the correction path; a `blocking`
/// entry should set its own and will be warned if it doesn't.
pub const DEFAULT_MAX_LATENCY_MS: u64 = 2_000;

/// A `blocking` entry above this is refused. Past roughly a fifth of a
/// second the user has started the next word, and a "correction" that
/// lands then is just corruption arriving late.
pub const MAX_BLOCKING_LATENCY_MS: u64 = 250;

/// Decided words remembered per plug-in, by default.
pub const DEFAULT_CACHE_SIZE: usize = 2_048;

/// How many queries may be waiting for the worker before new ones are
/// dropped. Small on purpose: a backlog means the endpoint is slower
/// than the user types, and yesterday's word is worthless.
pub const QUEUE_DEPTH: usize = 32;

/// `(preset name, endpoint, wire format)` for `provider = "..."`.
///
/// A preset only fills in what the entry leaves blank — shorthand for
/// two fields, never a special case downstream. Every URL here is one
/// the *user* has to be running or hold an account for; PolterType
/// ships no credentials and no default endpoint.
pub const PRESETS: &[(&str, &str, WireFormat)] = &[
    (
        "ollama",
        "http://127.0.0.1:11434/api/generate",
        WireFormat::OllamaGenerate,
    ),
    (
        "llama-cpp",
        "http://127.0.0.1:8080/v1/chat/completions",
        WireFormat::OpenAiChat,
    ),
    (
        "lm-studio",
        "http://127.0.0.1:1234/v1/chat/completions",
        WireFormat::OpenAiChat,
    ),
    (
        "openai",
        "https://api.openai.com/v1/chat/completions",
        WireFormat::OpenAiChat,
    ),
    (
        "anthropic",
        "https://api.anthropic.com/v1/messages",
        WireFormat::AnthropicMessages,
    ),
];

/// Anthropic requires a pinned API version header.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The instruction sent with every query.
///
/// Deliberately tiny: one word and the candidate readings of it, never
/// the surrounding sentence, the document or anything identifying. One
/// token back, which is cheap to parse and leaves no room for the model
/// to editorialise into our decision.
pub const SYSTEM_PROMPT: &str = "\
You identify which keyboard layout a typed word was meant for. \
You will be given numbered candidate readings of the same keystrokes \
under different layouts. Reply with the number of the reading that is \
a real word a human meant to type, or 0 if none of them is. \
Reply with the number only — no punctuation, no explanation.";
