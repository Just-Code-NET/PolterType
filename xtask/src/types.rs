//! Plain data shapes shared across the xtask commands.

use crate::enums::ExpandMode;

/// A single upstream Hunspell dictionary, as listed in
/// [`crate::consts::HUNSPELL_SOURCES`].
pub(crate) struct HunspellSource {
    /// Upstream file stem, e.g. `"de_DE_frami"` — what the `.dic` and
    /// `.aff` are called under `data/wordlists/sources/`.
    pub(crate) base: &'static str,
    pub(crate) dic: &'static str,
    pub(crate) aff: &'static str,
    /// Output file under `data/wordlists/`, e.g. `"de_de.txt.gz"`.
    pub(crate) output: &'static str,
    /// Whether to run the affix rules. [`ExpandMode::Full`] unless the
    /// dictionary's affix table is pathological — see the enum.
    pub(crate) expand: ExpandMode,
    /// Language-specific touch-ups to the processed list —
    /// [`NO_EXTRAS`](crate::consts::NO_EXTRAS) for nearly every source.
    pub(crate) extras: Extras,
}

/// Applied in field order after the `.dic` is processed. Every entry
/// names its reason where it is listed.
pub(crate) struct Extras {
    /// Attached to every form as well as kept off it: for a stems-only
    /// list whose language writes its article as a prefix, so `كتاب`
    /// also yields `الكتاب`. Without them the commonest form of a noun
    /// is the one the dictionary lacks.
    pub(crate) prefixes: &'static [&'static str],
    /// `(from, to)`: every form containing `from` is kept and also
    /// emitted with each `from` replaced by `to`. For a key that types
    /// in one code point what the dictionary spells in two.
    pub(crate) respell: &'static [(&'static str, &'static str)],
    /// Forms removed last: real but rare words that would shadow a far
    /// more common entry of another layout.
    pub(crate) drop: &'static [&'static str],
}
