//! Affix group / rule data.

use super::*;

#[derive(Debug)]
pub(crate) struct AffixGroup {
    /// `Y` in the block header — controls PFX × SFX combination.
    /// Parsed but unused: no cross-products are generated.
    #[allow(dead_code)]
    pub(crate) cross_product: bool,
    pub(crate) rules: Vec<AffixRule>,
}

#[derive(Debug)]
pub(crate) struct AffixRule {
    /// Number of *characters* (not bytes) to strip from the relevant
    /// end of the word. `0` if the rule's strip field is the literal
    /// string `"0"`.
    pub(crate) strip_chars: usize,
    /// Characters to append (SFX) or prepend (PFX). Empty string if
    /// the rule's add field is `"0"`.
    pub(crate) add: String,
    /// Continuation flags from the `<add>/<flags>` syntax — every rule
    /// under each of these is also applied to this rule's output.
    pub(crate) continuation: Vec<String>,
    /// Condition atoms matched against the word's relevant end
    /// (suffix end for SFX, prefix start for PFX).
    pub(crate) condition: Vec<CondAtom>,
    /// `true` if the source condition is the literal `.` (match any
    /// word), tracked so it cannot be confused with a length-0
    /// condition.
    pub(crate) unconditional: bool,
}
