//! Remote LLM-backed language detector.
//!
//! Gated behind the `remote` cargo feature *and* the
//! `[ai].allow_remote` runtime setting. Even with both on, the
//! detector only fires when the existing pipeline reports low
//! confidence (configurable per-detector). All calls are subject to
//! a `max_latency_ms` budget — anything slower is dropped, since the
//! engine should never block typing.

mod detector;
mod enums;

pub use detector::*;
pub use enums::*;
