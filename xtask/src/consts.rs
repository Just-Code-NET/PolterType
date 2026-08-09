//! Upstream dictionary source URLs.

use crate::enums::ExpandMode;
use crate::types::HunspellSource;

pub(crate) const EN_URL: &str =
    "https://raw.githubusercontent.com/dwyl/english-words/master/words_alpha.txt";

pub(crate) const UK_README_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/README_uk_UA.txt";

/// The Hunspell-derived bundled languages, one [`HunspellSource`] each.
///
/// Both the `.dic` (stems with flags) and the `.aff` (affix rules) are
/// downloaded and run through `hunspell::Aff::expand`; without the
/// expansion ~70 % of common verb and declension forms are missing —
/// see DECISIONS.md (2026-05-07).
///
/// The source stem is the upstream file name; the output keeps our
/// snake_case layout-stem convention. Keep in lock-step with
/// `poltertype-core/build.rs::LAYOUTS` — a language with a layout but
/// no entry here gets an empty FST and plausibility-only detection,
/// which is legal but rarely intended.
pub(crate) const HUNSPELL_SOURCES: &[HunspellSource] = &[
    HunspellSource {
        base: "uk_UA",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.aff",
        output: "uk_ua.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "ru_RU",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.aff",
        output: "ru_ru.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "de_DE_frami",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.aff",
        output: "de_de.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "es_ES",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.aff",
        output: "es_es.txt.gz",
        expand: ExpandMode::Full,
    },
    // French moved down a level upstream (`fr_FR/` → `fr_FR/
    // dictionaries/`) at some point after the bundled fr_fr.txt.gz
    // was generated. The old URLs 404, which the fetch reported on
    // stderr and then carried on past — see the failure tally in
    // `fetch_wordlists`, added so the next one of these can't pass
    // for a green run.
    HunspellSource {
        base: "fr",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/dictionaries/fr.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/dictionaries/fr.aff",
        output: "fr_fr.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "pl_PL",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/pl_PL/pl_PL.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/pl_PL/pl_PL.aff",
        output: "pl_pl.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "cs_CZ",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/cs_CZ/cs_CZ.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/cs_CZ/cs_CZ.aff",
        output: "cs_cz.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "el_GR",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/el_GR/el_GR.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/el_GR/el_GR.aff",
        output: "el_gr.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "he_IL",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/he_IL/he_IL.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/he_IL/he_IL.aff",
        output: "he_il.txt.gz",
        expand: ExpandMode::StemsOnly,
    },
    HunspellSource {
        base: "tr_TR",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/tr_TR/tr_TR.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/tr_TR/tr_TR.aff",
        output: "tr_tr.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "bg_BG",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/bg_BG/bg_BG.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/bg_BG/bg_BG.aff",
        output: "bg_bg.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "it_IT",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/it_IT/it_IT.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/it_IT/it_IT.aff",
        output: "it_it.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "pt_PT",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/pt_PT/pt_PT.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/pt_PT/pt_PT.aff",
        output: "pt_pt.txt.gz",
        expand: ExpandMode::Full,
    },
    HunspellSource {
        base: "pt_BR",
        dic: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/pt_BR/pt_BR.dic",
        aff: "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/pt_BR/pt_BR.aff",
        output: "pt_br.txt.gz",
        expand: ExpandMode::Full,
    },
];

/// ISO-8859-2 (Latin-2) high half, `0xA0..=0xFF` → Unicode. Generated
/// from Python's `iso-8859-2` codec rather than typed by hand.
/// `U+FFFD` marks a byte unassigned in the codepage.
pub(crate) const LATIN2_HIGH: [char; 96] = [
    '\u{00A0}', '\u{0104}', '\u{02D8}', '\u{0141}', '\u{00A4}', '\u{013D}', '\u{015A}', '\u{00A7}',
    '\u{00A8}', '\u{0160}', '\u{015E}', '\u{0164}', '\u{0179}', '\u{00AD}', '\u{017D}', '\u{017B}',
    '\u{00B0}', '\u{0105}', '\u{02DB}', '\u{0142}', '\u{00B4}', '\u{013E}', '\u{015B}', '\u{02C7}',
    '\u{00B8}', '\u{0161}', '\u{015F}', '\u{0165}', '\u{017A}', '\u{02DD}', '\u{017E}', '\u{017C}',
    '\u{0154}', '\u{00C1}', '\u{00C2}', '\u{0102}', '\u{00C4}', '\u{0139}', '\u{0106}', '\u{00C7}',
    '\u{010C}', '\u{00C9}', '\u{0118}', '\u{00CB}', '\u{011A}', '\u{00CD}', '\u{00CE}', '\u{010E}',
    '\u{0110}', '\u{0143}', '\u{0147}', '\u{00D3}', '\u{00D4}', '\u{0150}', '\u{00D6}', '\u{00D7}',
    '\u{0158}', '\u{016E}', '\u{00DA}', '\u{0170}', '\u{00DC}', '\u{00DD}', '\u{0162}', '\u{00DF}',
    '\u{0155}', '\u{00E1}', '\u{00E2}', '\u{0103}', '\u{00E4}', '\u{013A}', '\u{0107}', '\u{00E7}',
    '\u{010D}', '\u{00E9}', '\u{0119}', '\u{00EB}', '\u{011B}', '\u{00ED}', '\u{00EE}', '\u{010F}',
    '\u{0111}', '\u{0144}', '\u{0148}', '\u{00F3}', '\u{00F4}', '\u{0151}', '\u{00F6}', '\u{00F7}',
    '\u{0159}', '\u{016F}', '\u{00FA}', '\u{0171}', '\u{00FC}', '\u{00FD}', '\u{0163}', '\u{02D9}',
];

/// ISO-8859-7 (Greek) high half, `0xA0..=0xFF` → Unicode. Same
/// provenance and `U+FFFD` convention as [`LATIN2_HIGH`].
pub(crate) const GREEK_HIGH: [char; 96] = [
    '\u{00A0}', '\u{2018}', '\u{2019}', '\u{00A3}', '\u{20AC}', '\u{20AF}', '\u{00A6}', '\u{00A7}',
    '\u{00A8}', '\u{00A9}', '\u{037A}', '\u{00AB}', '\u{00AC}', '\u{00AD}', '\u{FFFD}', '\u{2015}',
    '\u{00B0}', '\u{00B1}', '\u{00B2}', '\u{00B3}', '\u{0384}', '\u{0385}', '\u{0386}', '\u{00B7}',
    '\u{0388}', '\u{0389}', '\u{038A}', '\u{00BB}', '\u{038C}', '\u{00BD}', '\u{038E}', '\u{038F}',
    '\u{0390}', '\u{0391}', '\u{0392}', '\u{0393}', '\u{0394}', '\u{0395}', '\u{0396}', '\u{0397}',
    '\u{0398}', '\u{0399}', '\u{039A}', '\u{039B}', '\u{039C}', '\u{039D}', '\u{039E}', '\u{039F}',
    '\u{03A0}', '\u{03A1}', '\u{FFFD}', '\u{03A3}', '\u{03A4}', '\u{03A5}', '\u{03A6}', '\u{03A7}',
    '\u{03A8}', '\u{03A9}', '\u{03AA}', '\u{03AB}', '\u{03AC}', '\u{03AD}', '\u{03AE}', '\u{03AF}',
    '\u{03B0}', '\u{03B1}', '\u{03B2}', '\u{03B3}', '\u{03B4}', '\u{03B5}', '\u{03B6}', '\u{03B7}',
    '\u{03B8}', '\u{03B9}', '\u{03BA}', '\u{03BB}', '\u{03BC}', '\u{03BD}', '\u{03BE}', '\u{03BF}',
    '\u{03C0}', '\u{03C1}', '\u{03C2}', '\u{03C3}', '\u{03C4}', '\u{03C5}', '\u{03C6}', '\u{03C7}',
    '\u{03C8}', '\u{03C9}', '\u{03CA}', '\u{03CB}', '\u{03CC}', '\u{03CD}', '\u{03CE}', '\u{FFFD}',
];
