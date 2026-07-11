//! Affix group / rule data.

use super::*;

#[derive(Debug)]
pub(crate) struct AffixGroup {
    /// `Y` in the block header — controls PFX × SFX combination.
    /// We don't generate cross-products today; this is parsed for
    /// fidelity / future use.
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
    /// Continuation flags from the `<add>/<flags>` syntax — every
    /// rule under each of these flags is also applied to this rule's
    /// output. Most of our dictionaries don't use this.
    pub(crate) continuation: Vec<String>,
    /// Condition atoms matched against the word's relevant end
    /// (suffix end for SFX, prefix start for PFX).
    pub(crate) condition: Vec<CondAtom>,
    /// `true` if the source condition is the literal `.` — match any
    /// word. We track this so a `.`-condition with zero atoms doesn't
    /// look like a length-0 condition that always matches.
    pub(crate) unconditional: bool,
}
