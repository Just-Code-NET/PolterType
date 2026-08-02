//! Optional AI subsystem: a socket for a model the **user** supplies.
//!
//! ## What this crate is, and what it deliberately is not
//!
//! It is an *interface*. PolterType ships no model weights, no vendor
//! SDK, and no default endpoint. [`detector::LlmDetector`] knows how
//! to phrase a question and read an answer; what answers is an Ollama
//! the user is running, an API they hold the key to, or a gateway of
//! their own — configured by them in `[[ai.plugins]]`, or by nobody,
//! which is the default.
//!
//! That is the whole design. Bundling a model would mean choosing a
//! vendor on the user's behalf and shipping megabytes most people
//! never asked for; bundling a client for one provider is the same
//! choice with extra steps. A socket is honest: it works with whatever
//! the user already trusts, and with nothing at all until they say
//! otherwise.
//!
//! Two extension shims (both declared in `poltertype-detect`):
//!
//! * `Detector` — adds another voice to the layout decision.
//!   [`detector::LlmDetector`] implements it.
//! * `WordRewriter` — operates *after* layout detection on the final
//!   text, for tricks like smart-capitalise.
//!
//! ## Privacy posture
//!
//! This is the only part of PolterType that can send what you typed
//! anywhere, so the gates are layered and each one is a real barrier
//! rather than a setting that merely looks like one:
//!
//! * The `ai` Cargo feature is **off by default** in `poltertype-app`.
//! * The `remote` cargo sub-feature is **off by default** even when
//!   `ai` is on. Without it no HTTP client is compiled in — `cargo
//!   tree` on a stock build shows no `reqwest` at all.
//! * `[ai].enabled` must be true at runtime.
//! * A **non-loopback** endpoint additionally needs
//!   `[ai].allow_remote`. An endpoint on `127.0.0.1` does not, because
//!   nothing leaves the machine — see [`locality`], the one place that
//!   distinction is decided, which fails closed on anything it cannot
//!   parse.
//! * API keys live in the OS keychain via `keyring`. A literal secret
//!   in `config.toml` is refused at construction, not used.
//! * Nothing typed is ever logged, and the decision cache stores
//!   hashes rather than text.
//!
//! There is still no telemetry and no code here that reports to us.
//! The only address this crate ever contacts is one the user wrote
//! down themselves.

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
// The whole module is the HTTP client's wrapper, so without the
// feature there is nothing for it to do — and leaving it compiled
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
