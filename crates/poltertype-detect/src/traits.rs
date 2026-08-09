//! The extension points of the pipeline: layout detectors,
//! post-correction word rewriters, and spelling-suggestion providers.

use poltertype_types::LayoutId;

use crate::enums::{RewriteVerdict, Verdict};
use crate::types::{DetectionContext, RewriteRequest, Suggestion};

/// A detector judges which keyboard layout the user *intended*: it
/// defers with `NoOpinion`, vetoes a switch with `Keep`, or requests
/// one with `Switch` (see [`Verdict`]).
///
/// The engine runs detectors in priority order and stops at the first
/// non-`NoOpinion`, which is what lets the dictionary detector say
/// "this is fine, don't ask anyone else" and keeps real prose words
/// from being switched.
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict;
}

/// A word rewriter operates *after* layout detection: it looks at the
/// final text and may suggest a different one — the
/// "smart-capitalise", "expand-acronym" kind of trick `poltertype-ai`
/// is meant to enable.
///
/// **Not yet consumed by the engine**: there is no rewriter stage, so
/// nothing calls this trait today. The gating design is in
/// `docs/AI.md`.
pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}

/// Proposes replacements for a token that is neither a wrong-layout
/// word nor a dictionary word — a plain typo. The engine shows the
/// results in the tooltip and types the chosen one back.
///
/// Same seam as [`Detector`]: the built-in [`crate::Suggester`] is
/// dictionary-driven, and the AI subsystem can plug a smarter provider
/// in with no engine change.
///
/// `typed_rendering` is the token as rendered under `layout`, original
/// capitalisation preserved. Implementations must never log it.
pub trait SuggestionProvider: Send + Sync {
    /// Is the token a known word of `layout`'s language? The engine
    /// gates the offer on this — a valid word never gets a tooltip.
    /// Lives on the provider (not the engine) so the answer tracks
    /// the same hot-swappable dictionary set the candidates come
    /// from — per-app wordlist profiles included.
    fn is_known(&self, layout: &LayoutId, typed_rendering: &str) -> bool;

    fn suggest(&self, layout: &LayoutId, typed_rendering: &str, max: usize) -> Vec<Suggestion>;
}
