//! Affix flag-type and condition-atom enums.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlagType {
    /// Default Hunspell mode — one ASCII char per flag.
    Ascii,
    /// `FLAG long` — two chars per flag, packed.
    Long,
    /// `FLAG UTF-8` — one Unicode char per flag (used by es_ES).
    Utf8,
    /// `FLAG num` — decimal flags separated by `,` (used by tr_TR,
    /// whose affix table is one flag per surface form and so needs
    /// far more than 65535 distinct flags).
    Num,
}

/// One atom of a Hunspell condition pattern.
#[derive(Debug)]
pub(crate) enum CondAtom {
    /// `.` — matches any single character.
    Any,
    /// Literal character — must match exactly.
    Char(char),
    /// `[abc]` — character must be one of these.
    Class(Vec<char>),
    /// `[^abc]` — character must NOT be one of these.
    NegClass(Vec<char>),
}
