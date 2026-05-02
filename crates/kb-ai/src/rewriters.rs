//! AI-backed [`WordRewriter`]s.
//!
//! v0.1 ships a single example rewriter that demonstrates the shape
//! of the API. Real implementations land in v0.1.x.

use kb_detect::{RewriteRequest, RewriteVerdict, WordRewriter};

/// Capitalises the first letter of common proper nouns. Demonstrative
/// stub — replace with an LLM-backed implementation in v0.1.x.
pub struct SmartCapitalize;

impl WordRewriter for SmartCapitalize {
    fn name(&self) -> &'static str {
        "smart-capitalize"
    }

    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict {
        let lower = req.original.to_lowercase();
        let names = [
            "github", "rust", "linux", "windows", "macos", "kyiv", "lviv",
        ];
        if names.contains(&lower.as_str()) {
            let mut chars = req.original.chars();
            if let Some(first) = chars.next() {
                let rest: String = chars.collect();
                let cap: String = first.to_uppercase().chain(rest.chars()).collect();
                if cap != req.original {
                    return RewriteVerdict::Replace {
                        text: cap,
                        reason: "capitalized known proper noun".into(),
                        require_confirmation: false,
                    };
                }
            }
        }
        RewriteVerdict::Keep
    }
}
