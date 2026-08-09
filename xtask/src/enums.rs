//! Choices the wordlist pipeline switches on.

/// Character encoding of a Hunspell dictionary pair, as declared by the
/// `SET` directive in the `.aff` — which covers **both** files, since
/// the `.dic` carries no `SET` of its own.
///
/// Reading each file's own bytes and defaulting to Latin-1 is what
/// silently mangled Polish and Greek into nonsense; German only
/// survived because German really is Latin-1.
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
    /// inflectional. Hebrew forced this to exist: `he_IL.aff` carries
    /// 3335 prefix rules and zero suffix rules, because it encodes the
    /// clitic prefixes and every legal pair of them as affixes.
    /// Expanding it faithfully yields 60.6 M forms — a 141 MB `.txt.gz`
    /// and a far larger FST in every installer, for a language whose
    /// script already separates it from every other bundled layout on
    /// sight.
    StemsOnly,
}
