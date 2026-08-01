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
}
