//! Choices the wordlist pipeline switches on.

/// Character encoding of a Hunspell dictionary pair, as declared by
/// the `SET` directive in the `.aff`.
///
/// The `.dic` carries no `SET` of its own — in Hunspell the `.aff`
/// declares the encoding for **both** files. Reading each file's own
/// bytes for a `SET` line and defaulting to Latin-1 when there isn't
/// one is what silently mangled Polish (ISO-8859-2) and Greek
/// (ISO-8859-7) into `bel\u{00F3}w`-shaped nonsense; German only
/// survived it because German really is Latin-1.
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
    /// Reserved for dictionaries whose affix table is combinatorial
    /// rather than inflectional. Hebrew is the case that forced this
    /// to exist: `he_IL.aff` carries 3335 prefix rules and *zero*
    /// suffix rules, because it encodes the clitic prefixes
    /// (ב ל כ מ ש ו ה, and every legal pair of them) as affixes.
    /// Expanding it faithfully yields 60.6 M forms — a 141 MB
    /// `.txt.gz` in the repo and a far larger FST inside every
    /// installer, for a language whose script already separates it
    /// from every other bundled layout on sight. Stems keep the
    /// dictionary useful as a refinement without paying that.
    StemsOnly,
}
