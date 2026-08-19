//! Choices the wordlist pipeline switches on.

/// Character encoding of a Hunspell dictionary pair, as declared by the
/// `SET` directive in the `.aff` — which covers **both** files, since
/// the `.dic` carries no `SET` of its own. See
/// [`crate::wordlists::detect_encoding`] for what guessing cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    /// ISO-8859-1, and Windows-1252 which differs from it only in
    /// 0x80–0x9F — a range our sources don't use.
    Latin1,
    /// ISO-8859-2 — Polish, Czech, Hungarian, Croatian.
    Latin2,
    /// ISO-8859-7 — Greek.
    Greek,
}

/// How much of a Hunspell dictionary to turn into surface forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpandMode {
    /// Run the affix rules — the default, and what every inflected
    /// language needs to cover the forms a user actually types.
    Full,
    /// Keep the `.dic` stems and skip affix expansion entirely.
    ///
    /// For dictionaries whose affix table is combinatorial rather than
    /// inflectional. Hebrew forced this to exist: `he_IL.aff` encodes
    /// clitic prefixes and every legal pair of them as affixes — 3335
    /// prefix rules, zero suffix rules — so a faithful expansion is
    /// 60.6 M forms and a 141 MB `.txt.gz` in every installer, for a
    /// language its script already separates on sight.
    StemsOnly,
}
