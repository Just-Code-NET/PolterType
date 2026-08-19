//! Token-shape helpers: canonicalisation and "is this even
//! prose?" guards shared by detectors and the engine.

/// Strip every non-letter character from `s` and lowercase it. "Letter"
/// is `char::is_alphabetic`, so digits, `'`, `-`, spaces and
/// punctuation all go — the output feeds a Hunspell-derived dictionary
/// that holds only pure-letter entries.
///
/// A buffer can hold a scancode rendering as punctuation in the current
/// layout and a letter in the alt one (0x27 → `;` in en-US, `ж` in
/// uk-UA), so the current render carries stray `;`s and would never hit
/// an entry. Stripping first keeps the detector honest.
pub fn letters_only_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_alphabetic() {
            for low in ch.to_lowercase() {
                out.push(low);
            }
        }
    }
    out
}

/// Count of characters that cannot be part of a word in ANY layout:
/// not alphabetic, and not one of the marks real words carry inside
/// them (apostrophe variants and the hyphen — the same set
/// [`surface_lower`] preserves; keep the two in sync).
///
/// The cross-layout artifact is what this measures. `mañana` typed with
/// en-US active renders `ma;ana`, whose letters-only skeleton `maana`
/// happens to be a dictionary entry — but real prose never carries a
/// `;` mid-token, so this count separates "the user typed this word"
/// from "the letters between the punctuation coincidentally spell one".
pub fn non_word_char_count(s: &str) -> usize {
    s.chars()
        .filter(|c| !c.is_alphabetic() && !matches!(c, '\'' | '’' | 'ʼ' | '-'))
        .count()
}

/// Suggestions-side canonicalisation: lowercase, keep letters plus
/// apostrophes and hyphens, fold `’` and `ʼ` to `'`, drop the rest.
///
/// [`letters_only_lower`] is deliberately lossy — membership lookup does
/// not care that `п'ять` lost its apostrophe. A *suggestion* does: it
/// gets typed back into the user's text. The surface FST is built by
/// `poltertype-core/build.rs` with a mirror of this function, so keep
/// the two in sync.
pub fn surface_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        // Apostrophes first: `ʼ` (U+02BC) is Unicode category Lm —
        // `is_alphabetic()` returns true for it, so the alphabetic
        // branch would keep it un-folded.
        if matches!(ch, '\'' | '’' | 'ʼ') {
            out.push('\'');
        } else if ch == '-' {
            out.push('-');
        } else if ch.is_alphabetic() {
            for low in ch.to_lowercase() {
                out.push(low);
            }
        }
    }
    out
}

/// Does `text` look like a programming identifier rather than prose?
/// When true the engine suppresses *automatic* switching for that
/// buffer — corrupting code is far worse than leaving a wrong-layout
/// token alone. The manual switch hotkey bypasses this filter.
///
/// Any one signal is enough: an underscore; a mid-token capital
/// (`getValue`); a letter+digit mix (`var2`); or code punctuation that
/// escaped the buffer's word-class table (backslash, semicolon,
/// backtick).
///
/// Acronyms (`URL`) and ordinary capitalised prose (`Привіт`)
/// deliberately do not trip it. The well-known acronyms live in
/// `data/wordlists/en_us-extras.txt` and are caught by the dictionary
/// detector first; this is the fallback for the long tail, capped at 5
/// letters so shouted prose goes through normal scoring instead.
/// Mixed letter+digit shapes like `H2O` go to `looks_like_code_token`.
pub fn looks_like_acronym(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() || letters.len() > 5 {
        return false;
    }
    // Non-letter, non-whitespace characters disqualify (`SQL;` ≠
    // acronym — let the code-token guard handle that).
    if text
        .chars()
        .any(|c| !c.is_alphabetic() && !c.is_whitespace())
    {
        return false;
    }
    letters.iter().all(|c| c.is_uppercase())
}

/// Minimum letters a compound segment needs before it may speak for the
/// whole token in the guards below. Two-letter segments are the noisy
/// end of both the FST and the shape scorer, and real hyphenated prose
/// leaves exactly such stubs — `будь-що` renders as `,elm-oj`, whose
/// `oj` scores a perfect en-US fit.
pub const COMPOUND_SEGMENT_MIN_LETTERS: usize = 3;

/// Split a hyphen- or dot-joined token into its segments, or `None`
/// when it is not a compound.
///
/// A compound is a token the wrong-layout hypothesis has to explain
/// piece by piece. Scoring the joined letters hides the structure:
/// `cqrs-client` reads as noise under en-US because of the acronym
/// glued to its front, so the Cyrillic rendering wins the whole token
/// on a segment the user never meant as a word.
///
/// `None` on an empty segment, so a leading/trailing separator or `--`
/// is never mistaken for structure.
pub fn compound_segments(text: &str) -> Option<Vec<&str>> {
    if !text.contains(['-', '.']) {
        return None;
    }
    let segments: Vec<&str> = text.split(['-', '.']).collect();
    if segments.iter().any(|s| s.is_empty()) {
        return None;
    }
    Some(segments)
}

/// Pair a compound's segments with the same buffer rendered through
/// another layout, `(current, alt)` per position.
///
/// `None` unless **both** renderings are compounds with the same number
/// of segments. Separators are not universal — `-` is `ß` under de-DE —
/// so a differing count means the two layouts disagree about where the
/// structure is, and the guards have nothing to compare. Refusing to
/// judge there is what keeps `Fußball` (typed in de-DE, rendered
/// `fu-ball` under en-US) correctable: its `ball` reads as a perfect
/// English word, and only the missing counterpart says so.
pub fn paired_segments<'a>(current: &'a str, alt: &'a str) -> Option<Vec<(&'a str, &'a str)>> {
    let (cur, alt) = (compound_segments(current)?, compound_segments(alt)?);
    (cur.len() == alt.len()).then(|| cur.into_iter().zip(alt).collect())
}

/// May this segment speak for its token? At least
/// [`COMPOUND_SEGMENT_MIN_LETTERS`] letters and no stray punctuation —
/// a segment carrying a stray is itself a cross-layout artifact, so
/// whatever it spells is coincidence. Same reasoning
/// [`non_word_char_count`] exists for.
pub fn segment_vouches(segment: &str) -> bool {
    non_word_char_count(segment) == 0
        && segment.chars().filter(|c| c.is_alphabetic()).count() >= COMPOUND_SEGMENT_MIN_LETTERS
}

pub fn looks_like_code_token(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let chars: Vec<char> = text.chars().collect();

    // 1. snake_case / leading underscore.
    if chars.contains(&'_') {
        return true;
    }

    // 4. Code punctuation kept in the buffer.
    if chars.iter().any(|c| matches!(*c, '\\' | ';' | '`')) {
        return true;
    }

    // 3. Letter + digit mix.
    let has_letter = chars.iter().any(|c| c.is_alphabetic());
    let has_digit = chars.iter().any(|c| c.is_ascii_digit());
    if has_letter && has_digit {
        return true;
    }

    // 2. Mid-token capital after a lowercase letter.
    let letters: Vec<char> = chars
        .iter()
        .copied()
        .filter(|c| c.is_alphabetic())
        .collect();
    for w in letters.windows(2) {
        if w[0].is_lowercase() && w[1].is_uppercase() {
            return true;
        }
    }

    false
}
