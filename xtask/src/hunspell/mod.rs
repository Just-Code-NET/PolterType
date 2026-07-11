//! Tiny Hunspell `.aff` parser + `.dic` expander.
//!
//! ## Why this exists
//!
//! Hunspell dictionaries store **stems**, not surface forms. The
//! `.dic` file lists each stem with a flag string (`мати/Z{P`,
//! `find/SDG`); the matching `.aff` file contains affix rules
//! per-flag that expand the stem into all inflected forms (`має`,
//! `матиму`, `finds`, `finding`, `found`, …). Without an expander,
//! ~70 % of the verbal vocabulary in any inflected language is
//! missing from the surface FST — exactly the gap that produced the
//! "має" auto-delete bug (DECISIONS.md, 2026-05-07).
//!
//! ## What it covers
//!
//! Enough Hunspell to expand the LibreOffice dictionaries we ship —
//! that means:
//!
//! * Suffix (`SFX`) and prefix (`PFX`) rules with simple
//!   `<strip> <add> <condition>` shape.
//! * Condition patterns: literal chars, `.` (any single char), `[abc]`
//!   class, `[^abc]` negative class.
//! * Three flag-encoding modes: default ASCII (one flag per char),
//!   `FLAG long` (two chars per flag), and `FLAG UTF-8` (one Unicode
//!   char per flag — same shape as ASCII at the parser level).
//! * Continuation flags inside `<add>/CONT` — recursively expanded
//!   with a depth cap to keep pathological cases bounded.
//!
//! ## What it does NOT cover
//!
//! * `FLAG num` (comma-separated decimal flags) — none of our
//!   dictionaries use it; we error out if encountered so the build
//!   fails loudly rather than silently mis-expanding.
//! * `COMPOUND*` rules (compound-word generation). Compounds are a
//!   tiny fraction of inflected forms in our target languages and
//!   the engine doesn't need them — wrong-layout detection works
//!   on individual word boundaries, never on multi-stem compounds.
//! * Cross-product PFX × SFX combinations. uk_UA has zero PFX rules,
//!   the others have a handful — generating each one's PFX-only and
//!   SFX-only forms is enough vocabulary in practice. We skip the
//!   cross to keep the expander small and the FST size bounded.
//! * `ICONV` / `OCONV` input/output character conversions. Those
//!   are spell-checker concerns (normalising user input before
//!   lookup); we generate canonical forms only.
//! * `MAP`, `REP`, `BREAK`, `KEY`, `TRY`, `WORDCHARS`, `IGNORE`,
//!   `NEEDAFFIX`, `CIRCUMFIX`, `ONLYINCOMPOUND`, …  — also
//!   spell-checker-side. We ignore them while parsing.
//!
//! Trade-off: this is a **lossy** Hunspell port — the resulting FST
//! covers most surface vocabulary the user will type in prose, but
//! not the corner cases (compound nouns in German that aren't listed
//! as separate stems, deep cross-product chains, etc.). The
//! `data/wordlists/<lang>-extras.txt` overlay is the escape hatch
//! when a missing form bites.

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
