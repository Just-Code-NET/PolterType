//! Upstream dictionary source URLs.

pub(crate) const EN_URL: &str =
    "https://raw.githubusercontent.com/dwyl/english-words/master/words_alpha.txt";

pub(crate) const UK_README_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/README_uk_UA.txt";

// Hunspell-derived sources for the bundled languages. We download the
// `.dic` (word stems with flags) AND the matching `.aff` (affix
// rules), then run them through `hunspell::Aff::expand` to get full
// inflected surface forms. Without this, ~70 % of common verb /
// declension forms are missing — see DECISIONS.md (2026-05-07) and
// `xtask/src/hunspell.rs` for the full story.
//

// URLs spelled out in full instead of concatenated from a base —
// `concat!` only takes literals and a const-fn helper would obscure
// what's a plain list of file paths.
pub(crate) const UK_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.dic";

pub(crate) const UK_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.aff";

pub(crate) const RU_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.dic";

pub(crate) const RU_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.aff";

pub(crate) const DE_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.dic";

pub(crate) const DE_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.aff";

pub(crate) const ES_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.dic";

pub(crate) const ES_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.aff";

pub(crate) const FR_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/fr.dic";

pub(crate) const FR_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/fr.aff";
