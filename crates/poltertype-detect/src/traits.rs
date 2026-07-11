//! The two extension points of the pipeline: layout detectors
//! and post-correction word rewriters.

use crate::enums::{RewriteVerdict, Verdict};
use crate::types::{DetectionContext, RewriteRequest};

/// A detector judges which keyboard layout the user *intended*.
///
/// Three possible outcomes (see [`Verdict`]):
///
/// * `NoOpinion` — defer to the next detector in the pipeline.
/// * `Keep` — actively veto a switch (used by the dictionary when
///   the current text is already a valid word).
/// * `Switch` — request a layout change.
///
/// The engine runs detectors in priority order and stops at the
/// first non-`NoOpinion`. This is the load-bearing change that lets
/// the dictionary detector say "this is fine, don't ask anyone else"
/// — vital for avoiding false positives on real prose words.
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict;
}

/// A word rewriter operates *after* layout detection: it looks at the
/// final text and may suggest a different one. Intended for the
/// "smart-capitalize", "expand-acronym", "slang-to-formal" kind of
/// power-user tricks the AI subsystem (see `poltertype-ai`) is meant
/// to enable.
///
/// **Not yet consumed by the engine**: there is no rewriter stage in
/// `poltertype-core`, so nothing calls this trait today. The gating
/// design (a settings flag plus the per-rewriter
/// `require_confirmation` toggle) is described in `docs/AI.md`.
pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}
