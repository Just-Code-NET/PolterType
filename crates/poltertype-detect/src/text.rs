//! Token-shape helpers: canonicalisation and "is this even
//! prose?" guards shared by detectors and the engine.

/// Strip every non-letter (`char::is_alphabetic`) from `s` and
/// lowercase it, for the Hunspell-derived dictionaries that hold only
/// pure-letter entries.
///
/// A scancode can render as punctuation in the current layout and a
/// letter in the alt one (0x27 → `;` in en-US, `ж` in uk-UA), so an
/// un-stripped wrong-layout render would never hit an entry.
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
/// not alphabetic, and not an apostrophe variant or hyphen — the same
/// set [`surface_lower`] preserves; keep the two in sync.
///
/// This measures the cross-layout artifact: `mañana` typed with en-US
/// active renders `ma;ana`, whose letters-only skeleton `maana` is a
/// dictionary entry — but real prose never carries a `;` mid-token.
pub fn non_word_char_count(s: &str) -> usize {
    s.chars()
        .filter(|c| !c.is_alphabetic() && !matches!(c, '\'' | '’' | 'ʼ' | '-'))
        .count()
}

/// Suggestions-side canonicalisation: lowercase, keep letters plus
/// apostrophes and hyphens, fold `’` and `ʼ` to `'`, drop the rest.
///
/// [`letters_only_lower`] is deliberately lossy — membership lookup does
/// not care that `п'ять` lost its apostrophe, but a *suggestion* gets
/// typed back into the user's text. `poltertype-core/build.rs` builds
/// the surface FST with a mirror of this function; keep the two in sync.
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

/// All-caps token of at most 5 letters and nothing else. The well-known
/// acronyms live in `data/wordlists/en_us-extras.txt` and are caught by
/// the dictionary detector first; this is the fallback for the long
/// tail. The cap keeps shouted prose (`ПРИВІТ`) on normal scoring, and
/// shapes like `SQL;` or `H2O` go to [`looks_like_code_token`].
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

/// Minimum letters a compound segment needs before it may speak for its
/// whole token. Two-letter segments are the noisy end of both the FST
/// and the shape scorer, and real hyphenated prose leaves exactly such
/// stubs — `будь-що` renders as `,elm-oj`, whose `oj` scores a perfect
/// en-US fit.
pub const COMPOUND_SEGMENT_MIN_LETTERS: usize = 3;

/// Split a hyphen- or dot-joined token into its segments, or `None`
/// when it is not a compound — including on an empty segment, so a
/// leading/trailing separator or `--` is never read as structure.
///
/// A compound is a token the wrong-layout hypothesis has to explain
/// piece by piece; see DECISIONS.md (2026-08-20).
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
/// so a differing count means the layouts disagree about where the
/// structure is, and the guards have nothing to compare. Refusing to
/// judge there keeps `Fußball` (rendered `fu-ball` under en-US, whose
/// `ball` reads as perfect English) correctable.
pub fn paired_segments<'a>(current: &'a str, alt: &'a str) -> Option<Vec<(&'a str, &'a str)>> {
    let (cur, alt) = (compound_segments(current)?, compound_segments(alt)?);
    (cur.len() == alt.len()).then(|| cur.into_iter().zip(alt).collect())
}

/// May this segment speak for its token? At least
/// [`COMPOUND_SEGMENT_MIN_LETTERS`] letters and no stray punctuation —
/// a segment carrying a stray is itself a cross-layout artifact, so
/// whatever it spells is coincidence.
pub fn segment_vouches(segment: &str) -> bool {
    non_word_char_count(segment) == 0
        && segment.chars().filter(|c| c.is_alphabetic()).count() >= COMPOUND_SEGMENT_MIN_LETTERS
}

/// Does `text` look like a programming identifier rather than prose?
/// Any one signal is enough: an underscore, a mid-token capital
/// (`getValue`), a letter+digit mix (`var2`), or code punctuation that
/// escaped the buffer's word-class table. Acronyms (`URL`) and ordinary
/// capitalised prose (`Привіт`) deliberately do not trip it.
///
/// When true the engine suppresses *automatic* switching for that
/// buffer — corrupting code is far worse than leaving a wrong-layout
/// token alone. The manual switch hotkey bypasses this filter.
pub fn looks_like_code_token(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let chars: Vec<char> = text.chars().collect();

    if chars.contains(&'_') {
        return true;
    }

    if chars.iter().any(|c| matches!(*c, '\\' | ';' | '`')) {
        return true;
    }

    let has_letter = chars.iter().any(|c| c.is_alphabetic());
    let has_digit = chars.iter().any(|c| c.is_ascii_digit());
    if has_letter && has_digit {
        return true;
    }

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
