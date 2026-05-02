//! Optional AI subsystem.
//!
//! Implements `Detector` and `WordRewriter` plug-ins backed by local ONNX
//! models or remote LLM APIs. Off by default — gated behind the `ai`
//! feature flag in `kb-app`.
//!
//! Phase 1 placeholder. Real implementation lands in Phase 7.

#![forbid(unsafe_code)]
