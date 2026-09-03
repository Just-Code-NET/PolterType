//! Crate-wide constants: fuzzy-suggestion ranking, plus the compound
//! segment threshold shared by [`crate::text`] and the dictionary
//! detector.

/// Minimum letters a compound segment needs before it may speak for its
/// whole token. Two-letter segments are the noisy end of both the FST
/// and the shape scorer, and real hyphenated prose leaves exactly such
/// stubs — `будь-що` renders as `,elm-oj`, whose `oj` scores a perfect
/// en-US fit.
pub const COMPOUND_SEGMENT_MIN_LETTERS: usize = 3;

/// Cap on raw candidates pulled off the FST stream before ranking —
/// a d=2 automaton over a short token can match thousands of words,
/// and ranking more than this many never changes the visible top-N.
pub(crate) const MAX_RAW_CANDIDATES: usize = 500;

/// Minimum alphabetic length of a token we suggest for. Below this
/// the candidate space is all noise (`cat` is distance 2 from `a`).
pub(crate) const MIN_TOKEN_LETTERS: usize = 3;

/// Minimum alphabetic length for the distance-2 pass.
pub(crate) const MIN_LETTERS_FOR_D2: usize = 5;

/// Ranking scores above this are dropped even if we found nothing
/// better: suggesting a far-away word is worse than staying quiet.
pub(crate) const MAX_SCORE: f32 = 2.6;

/// Transposition cost (`тичба` → `тиба`… `таби`) — swapped fingers.
pub(crate) const TRANSPOSITION_COST: f32 = 0.6;

/// Penalty when the first letters differ (typos rarely start a word)
/// and a smaller one for the last letter.
pub(crate) const FIRST_CHAR_PENALTY: f32 = 0.3;
pub(crate) const LAST_CHAR_PENALTY: f32 = 0.15;

/// Penalty for words on the curated weak list (archaic vocatives,
/// dead inflections) — valid, but almost never what the user meant.
pub(crate) const WEAK_PENALTY: f32 = 0.6;

/// Reward per shared leading character, capped — breaks ties towards
/// candidates that start the way the user actually typed.
pub(crate) const PREFIX_BONUS: f32 = 0.08;
pub(crate) const PREFIX_BONUS_CAP: usize = 4;
