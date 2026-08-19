//! Optional AI subsystem: a socket for a model the **user** supplies.
//!
//! It is an *interface*. PolterType ships no model weights, no vendor
//! SDK and no default endpoint. [`detector::LlmDetector`] knows how to
//! phrase a question and read an answer; what answers is an Ollama the
//! user runs, an API they hold the key to, or a gateway of their own —
//! or nobody, which is the default. Bundling a model would choose a
//! vendor on the user's behalf; bundling one provider's client is the
//! same choice with extra steps.
//!
//! Two extension shims, both declared in `poltertype-detect`:
//! `Detector`, another voice in the layout decision, implemented by
//! [`detector::LlmDetector`]; and `WordRewriter`, which operates after
//! layout detection on the final text.
//!
//! **Privacy posture.** This is the only part of PolterType that can
//! send what you typed anywhere, so four gates stack, each a real
//! barrier: the `ai` Cargo feature (off by default); the `remote`
//! sub-feature, without which no HTTP client is compiled in at all;
//! `[ai].enabled` at runtime; and `[ai].allow_remote` for a
//! **non-loopback** endpoint, decided in [`locality`], which fails
//! closed on anything it cannot parse. API keys live in the OS
//! keychain — a literal secret in `config.toml` is refused at
//! construction, not used. Nothing typed is logged and the decision
//! cache stores hashes, not text. No telemetry: the only address this
//! crate ever contacts is one the user wrote down. See `docs/AI.md`.

#![forbid(unsafe_code)]
// Same test-only allowance the other crates carry: a test that cannot
// panic cannot assert. See `poltertype-update/src/lib.rs`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod detector;
pub mod factory;
pub mod locality;
pub mod rewriters;
pub mod wire;

mod cache;
mod consts;
mod enums;
mod keys;
// The whole module wraps the HTTP client, so without the feature it
// would be dead code that only a `--no-default-features` lint run
// notices.
#[cfg(feature = "remote")]
mod transport;
mod types;

pub use consts::*;
pub use enums::*;
pub use factory::build_detectors;
pub use keys::*;
pub use types::*;
