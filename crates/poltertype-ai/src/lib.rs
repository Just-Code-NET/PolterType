//! Optional AI subsystem: LLM-backed [`Detector`]s and
//! [`WordRewriter`]s.
//!
//! Two extension shims (both already declared in `poltertype-detect`):
//!
//! * `Detector` — adds another voice to the layout-decision pipeline.
//!   Local ONNX models (`local::LocalOnnxDetector`, stub in v0.1) and
//!   remote LLMs (`remote::RemoteLlmDetector`, gated behind the
//!   `remote` feature) implement it.
//! * `WordRewriter` — operates *after* layout detection on the final
//!   text. Used for power-user tricks like smart-capitalize or
//!   expand-acronym.
//!
//! ## Privacy posture (DECISIONS.md, §3.8 of PLAN.md)
//!
//! * The whole `ai` Cargo feature is **off by default** in `poltertype-app`.
//! * The `remote` cargo sub-feature is **off by default** even when
//!   `ai` is on; it adds `reqwest` and TLS to the build.
//! * Even when `ai = enabled` and `remote` is built in, the
//!   `[ai].allow_remote` settings flag must also be true at runtime.
//!   Two switches by design.
//! * API keys are stored in the OS keychain via `keyring`, never in
//!   `config.toml`.

#![forbid(unsafe_code)]
// Same test-only allowance the other crates carry: a test that cannot
// panic cannot assert. See `poltertype-update/src/lib.rs`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod factory;
pub mod local;
pub mod remote;
pub mod rewriters;

mod enums;
mod keys;
mod types;

pub use enums::*;
pub use factory::build_detectors;
pub use keys::*;
pub use types::*;
