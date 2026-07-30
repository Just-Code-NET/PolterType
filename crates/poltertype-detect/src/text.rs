//! Token-shape helpers: canonicalisation and "is this even
//! prose?" guards shared by detectors and the engine.

/// Strip every non-letter character from `s` and return a lowercase
/// `String`. "Letter" here is `char::is_alphabetic`, so digits / `'` /
/// `-` / spaces / punctuation are all dropped — the function is
/// designed to feed clean tokens into a Hunspell-derived dictionary,
/// which only contains pure-letter entries.
///
/// The motivating case: with the cross-layout-letter buffer hint, a
/// buffer can contain a scancode whose *current* layout renders as
/// punctuation but whose *alt* layout is a letter (0x27 → `;` in
/// en-US, `ж` in uk-UA). The current-render then has stray `;`s
/// mid-string and would never hit a dictionary entry. Stripping
/// before lookup keeps the detector honest: if the *letter*
/// substring is a real word, the verdict reflects that.
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

/// Count of characters in `s` that cannot be part of a word in ANY
/// layout: not alphabetic and not one of the marks real words carry
/// inside them (the apostrophe variants and the hyphen — the same
/// set [`surface_lower`] preserves; keep the two in sync).
///
/// The motivating case is the cross-layout artifact. Scancode 0x27
/// renders as `;` under en-US but `ñ` under es-ES (and `ж` under
/// uk-UA), so a user typing `mañana` with the wrong layout active
/// produces the current-render `ma;ana`. Its letters-only skeleton
/// `maana` happens to be an embedded-dictionary entry — and a real
/// prose word never carries a `;` mid-token — so detectors use this
/// count to tell "the user really typed this word" apart from "the
/// letters between the punctuation coincidentally spell one".
pub fn non_word_char_count(s: &str) -> usize {
    s.chars()
        .filter(|c| !c.is_alphabetic() && !matches!(c, '\'' | '’' | 'ʼ' | '-'))
        .count()
}

/// Suggestions-side canonicalisation: lowercase, keep letters plus
/// apostrophes and hyphens, fold the apostrophe variants (`’` U+2019,
/// `ʼ` U+02BC) to `'`. Everything else is dropped.
///
/// [`letters_only_lower`] is deliberately lossy — membership lookup
/// doesn't care that `п'ять` lost its apostrophe. A *suggestion* does:
/// it gets typed back into the user's text, so the surface FST (built
/// by `poltertype-core/build.rs` with a mirror of this function — keep
/// the two in sync) stores `п'ять` verbatim, and queries against it
/// must canonicalise the same way.
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

/// Heuristic: does `text` look like a programming-language identifier
/// rather than natural-language prose? When this returns `true`, the
/// engine suppresses *automatic* layout-switching for that buffer —
/// it would be far more annoying to corrupt a piece of code than to
/// leave a wrong-layout token alone. The user's manual switch hotkey
/// (`Ctrl+Shift+Backspace`) bypasses this filter.
///
/// Signals (any single one is enough):
///
/// 1. Underscore (`_`) — snake_case is rare in EN/UK prose.
/// 2. Mid-token capital letter (`getValue`, `MyClass`) — never in
///    prose; common in camelCase / PascalCase identifiers.
/// 3. Letter+digit mix (`var2`, `addr1`) — rare in prose, common in
///    versions / symbol names.
/// 4. Code punctuation that escaped the buffer's word-class table:
///    backslash, semicolon, backtick.
///
/// Acronyms (`URL`, `HTML`) and ordinary capitalised prose
/// (`Hello`, `Привіт`) deliberately do NOT trip the heuristic.
/// Acronym shape: a short all-uppercase alphabetic token.
///
/// Used by the plausibility detector as a safety net for SQL / IDE /
/// CLI / etc. that aren't in the embedded English dictionary. The
/// well-known ones (URL, HTML, API, JSON, …) live in
/// `data/wordlists/en_us-extras.txt` and are caught by the dict
/// detector first; this function is the fallback for the long tail.
///
/// Length cap: 5 letters. Real acronyms are almost always ≤5 chars
/// (HTTPS is the famous outlier). Anything longer (`HELLO`, `ПРИВІТ`)
/// might just be shouted prose, where mis-keying is more likely than
/// a deliberate caps acronym — let the normal plausibility pipeline
/// decide.
///
/// All-letters requirement: `H2O`-style mixed letter+digit goes to
/// `looks_like_code_token` instead.
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
