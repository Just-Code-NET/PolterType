//! Local on-device language-detection model.
//!
//! v0.1 ships a stub that respects the [`AiPluginConfig`] schema but
//! always returns `None` (no verdict). Real ONNX / Candle inference
//! is a Phase 7.x add — pulling those crates is heavy and we want
//! `cargo check --workspace` to stay quick for everyone.

use std::path::PathBuf;

use kb_detect::{Detector, DetectionContext, DetectionVerdict};
use tracing::warn;

use crate::AiError;

pub struct LocalOnnxDetector {
    pub id: String,
    pub model_path: PathBuf,
}

impl LocalOnnxDetector {
    pub fn new(id: String, model_path: PathBuf) -> Result<Self, AiError> {
        if !model_path.exists() {
            return Err(AiError::ModelMissing(model_path));
        }
        Ok(Self { id, model_path })
    }
}

impl Detector for LocalOnnxDetector {
    fn name(&self) -> &'static str {
        // Returning a static str isn't quite right with a runtime id,
        // but the architecture treats `name()` as a backend tag, not
        // an instance identifier.
        "local-onnx"
    }

    fn detect(&self, _ctx: &DetectionContext<'_>) -> Option<DetectionVerdict> {
        warn!(id = %self.id, "local ONNX detector is a stub; returning no verdict");
        None
    }
}
