//! Plain data passed through the pipeline: detection context,
//! rewrite requests, and per-layout linguistic profiles.

use std::collections::HashSet;

use poltertype_types::LayoutId;

use crate::enums::Script;

#[derive(Debug, Clone, Copy)]
pub struct RewriteRequest<'a> {
    pub original: &'a str,
    pub layout: &'a LayoutId,
    pub recent_context: &'a str,
}

/// Engine-supplied context: the buffer already rendered through every
/// candidate layout, so detectors don't depend on layout-mapping types.
#[derive(Debug, Clone)]
pub struct DetectionContext<'a> {
    pub current_layout: &'a LayoutId,
    /// `(layout, text rendered through that layout's mapping)`.
    pub candidates: &'a [(LayoutId, String)],
    pub recent_context: &'a str,
}

impl DetectionContext<'_> {
    pub fn text_for(&self, layout: &LayoutId) -> Option<&str> {
        self.candidates
            .iter()
            .find(|(l, _)| l == layout)
            .map(|(_, t)| t.as_str())
    }
}

/// Tiny per-layout linguistic profile used by the plausibility scorer.
/// Loaded from the layout-mapping TOMLs in `poltertype-core::layouts`.
#[derive(Debug, Clone)]
pub struct LayoutProfile {
    pub id: LayoutId,
    pub script: Script,
    /// Lowercase vowel chars for this layout's language. Real words
    /// have characteristic vowel ratios; noise typed through the wrong
    /// layout usually doesn't.
    pub vowels: HashSet<char>,
}

impl LayoutProfile {
    pub fn new(id: LayoutId, script: Script, vowels: impl IntoIterator<Item = char>) -> Self {
        Self {
            id,
            script,
            vowels: vowels.into_iter().collect(),
        }
    }
}
