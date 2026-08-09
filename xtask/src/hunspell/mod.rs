//! Tiny Hunspell `.aff` parser + `.dic` expander.
//!
//! Hunspell dictionaries store **stems**, not surface forms: the `.dic`
//! lists each stem with a flag string and the `.aff` holds the affix
//! rules that expand it. Without an expander ~70 % of the verbal
//! vocabulary in any inflected language is missing from the surface FST
//! — the gap behind the `має` auto-delete bug (DECISIONS.md,
//! 2026-05-07).
//!
//! Covers enough Hunspell to expand the LibreOffice dictionaries we
//! ship: `SFX`/`PFX` rules of `<strip> <add> <condition>` shape;
//! conditions with literals, `.`, `[abc]` and `[^abc]`; all four
//! flag-encoding modes, including `FLAG num` for tr_TR; and
//! continuation flags, recursively expanded under a depth cap.
//!
//! Deliberately absent: `COMPOUND*` (wrong-layout detection works on
//! word boundaries, never on multi-stem compounds), cross-product
//! PFX × SFX (uk_UA has no PFX rules and the others a handful, so
//! separate expansions are enough vocabulary), and the spell-checker
//! concerns — `ICONV`/`OCONV`, `MAP`, `REP`, `BREAK`, `TRY` and the
//! rest — which are ignored while parsing.
//!
//! This is therefore a **lossy** port: the FST covers most prose
//! vocabulary but not the corner cases. `data/wordlists/<lang>-extras.txt`
//! is the escape hatch when a missing form bites.

mod aff;
mod enums;
mod rules;
mod types;

pub use aff::*;
pub(crate) use enums::*;
pub(crate) use rules::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
