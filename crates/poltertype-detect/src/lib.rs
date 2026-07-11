//! Language detection pipeline.
//!
//! Pluggable: the engine holds `Vec<Box<dyn Detector>>` and runs them
//! in priority order. The first verdict whose confidence clears the
//! engine's threshold wins.
//!
//! v0.1 ships [`WordPlausibilityDetector`]. Real n-gram / dictionary /
//! ML detectors arrive with the AI subsystem (Phase 7) without any
//! trait change.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod dictionary;
mod enums;
mod plausibility;
mod text;
mod traits;
mod types;

pub use poltertype_types::{DetectionInput, DetectionVerdict, LayoutId};

pub use dictionary::{DictionaryDetector, LayoutDictionary};
pub use enums::{RewriteVerdict, Script, Verdict};
pub use plausibility::WordPlausibilityDetector;
pub use text::{letters_only_lower, looks_like_acronym, looks_like_code_token};
pub use traits::{Detector, WordRewriter};
pub use types::{DetectionContext, LayoutProfile, RewriteRequest};

#[cfg(test)]
mod tests;
