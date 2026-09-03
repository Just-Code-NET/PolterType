//! Language detection pipeline.
//!
//! Pluggable: the engine holds `Vec<Box<dyn Detector>>` and runs them in
//! priority order, stopping at the first verdict that is not
//! `NoOpinion` and clears the engine's confidence threshold.
//!
//! Two detectors ship here — [`DictionaryDetector`] first, then
//! [`WordPlausibilityDetector`] as the fallback. `poltertype-ai` adds a
//! third behind a feature flag, through the same trait.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod consts;
mod dictionary;
mod dictionary_detector;
mod distance;
mod enums;
mod geometry;
mod lev_automaton;
mod plausibility;
mod suggester;
mod text;
mod traits;
mod types;

pub use poltertype_types::{DetectionInput, DetectionVerdict, LayoutId};

pub use consts::COMPOUND_SEGMENT_MIN_LETTERS;
pub use dictionary::LayoutDictionary;
pub use dictionary_detector::DictionaryDetector;
pub use enums::{RewriteVerdict, Script, Verdict};
pub use geometry::KeyboardGeometry;
pub use plausibility::WordPlausibilityDetector;
pub use suggester::Suggester;
pub use text::{
    compound_segments, letters_only_lower, looks_like_acronym, looks_like_code_token,
    non_word_char_count, paired_segments, segment_vouches, surface_lower,
};
pub use traits::{Detector, SuggestionProvider, WordRewriter};
pub use types::{DetectionContext, LayoutProfile, RewriteRequest, Suggestion};

#[cfg(test)]
mod tests;
