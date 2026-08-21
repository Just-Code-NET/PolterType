use std::collections::{HashMap, HashSet};

use fst::Set as FstSet;

use super::*;

fn detector() -> WordPlausibilityDetector {
    let en = LayoutProfile::new(
        LayoutId::from("en-US"),
        Script::Latin,
        ['a', 'e', 'i', 'o', 'u', 'y'],
    );
    let uk = LayoutProfile::new(
        LayoutId::from("uk-UA"),
        Script::Cyrillic,
        ['а', 'е', 'и', 'і', 'о', 'у', 'ю', 'я', 'є', 'ї'],
    );
    let mut profiles = HashMap::new();
    profiles.insert(en.id.clone(), en);
    profiles.insert(uk.id.clone(), uk);
    WordPlausibilityDetector::new(profiles)
}

fn ctx<'a>(current: &'a LayoutId, cands: &'a [(LayoutId, String)]) -> DetectionContext<'a> {
    DetectionContext {
        current_layout: current,
        candidates: cands,
        recent_context: "",
    }
}

fn assert_switches_to(detector: &impl Detector, ctx: &DetectionContext<'_>, expected: &LayoutId) {
    match detector.judge(ctx) {
        Verdict::Switch(v) => assert_eq!(&v.best_layout, expected),
        other => panic!("expected Switch, got {other:?}"),
    }
}

fn assert_no_opinion(detector: &impl Detector, ctx: &DetectionContext<'_>) {
    assert!(matches!(detector.judge(ctx), Verdict::NoOpinion));
}

/// Regression: `kubectl` is a word developers type but is not in
/// `dwyl/english-words`, and the old any-advantage rule replaced it
/// with `лгиусед`. At `keep_threshold = 0.7` plausibility vetoes the
/// switch, because `kubectl` reads perfectly plausibly under en-US.
#[test]
fn plausibility_keeps_real_looking_uncommon_word() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "kubectl".into()),
        (uk.clone(), "лгиусед".into()),
    ];
    match detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for kubectl, got {other:?}"),
    }
}

#[test]
fn switches_for_typical_punto_case() {
    // user is in uk-UA, typed scancodes for "hello" → uk renders
    // them as "руддщ", en renders them as "hello".
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "hello".into()), (uk.clone(), "руддщ".into())];
    assert_switches_to(&detector(), &ctx(&uk, &cands), &en);
}

#[test]
fn switches_in_reverse_direction_too() {
    // user in en-US typed scancodes for "привіт" → en renders
    // garbage, uk renders properly.
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "ghbdsn".into()), (uk.clone(), "привіт".into())];
    assert_switches_to(&detector(), &ctx(&en, &cands), &uk);
}

#[test]
fn keeps_current_when_text_already_native() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "hello".into()), (uk.clone(), "руддщ".into())];
    // `hello` scores ≥ keep_threshold for en-US, so the detector
    // vetoes (Keep) rather than merely abstaining (NoOpinion).
    match detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep, got {other:?}"),
    }
}

#[test]
fn does_not_switch_for_short_buffer() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "ab".into()), (uk.clone(), "фи".into())];
    assert_no_opinion(&detector(), &ctx(&en, &cands));
}

/// Regression: a domain typed correctly under en-US was "corrected"
/// into Cyrillic. `.` is `ю` in uk-UA, so the host stays one token and
/// the renderings are asymmetric — en-US keeps the dots and paid two
/// stray-punctuation penalties, scoring 0.00 against 0.75. Scoring the
/// compound segment-wise is the fix.
#[test]
fn plausibility_keeps_hostname_typed_in_its_own_layout() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "games.just-code.net".into()),
        (uk.clone(), "пфьуіюогіе-сщвуютуе".into()),
    ];
    match detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for a hostname, got {other:?}"),
    }
}

/// The other half of the same fix: a *genuine* wrong-layout word
/// whose en-US rendering carries a dot because `ю` sits on the `.`
/// key must still be corrected. `союз` → `cj.p`: segment-wise
/// scoring leaves `cj` and `p` reading as nothing, so the compound
/// never earns the keep-veto a real hostname does.
#[test]
fn plausibility_still_switches_cyrillic_word_whose_render_has_a_dot() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "cj.p".into()), (uk.clone(), "союз".into())];
    assert_switches_to(&detector(), &ctx(&en, &cands), &uk);
}

/// A dot next to *other* stray punctuation is a wrong-layout
/// rendering, not compound structure — `любов` → `k.,jd` carries a
/// comma too, so the ordinary scoring path (and its stray penalty)
/// must still apply.
#[test]
fn plausibility_treats_dot_beside_other_punctuation_as_wrong_layout() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "k.,jd".into()), (uk.clone(), "любов".into())];
    assert_switches_to(&detector(), &ctx(&en, &cands), &uk);
}

/// A compound is only as good as its worst segment, and leading /
/// trailing / doubled dots are not compound structure at all.
#[test]
fn plausibility_fit_scores_compound_by_worst_segment() {
    let d = detector();
    let en = LayoutId::from("en-US");
    let whole = d.fit(&en, "games.net").expect("profile");
    let worst = d.fit(&en, "games.wsz").expect("profile");
    assert!(whole > 0.9, "every segment reads as a word: {whole}");
    assert!(
        worst < d.keep_threshold,
        "one unreadable segment must sink the compound: {worst}"
    );
    // Trailing dot: not a compound, so the stray term still bites.
    let trailing = d.fit(&en, "yjdj.").expect("profile");
    assert!(trailing < d.keep_threshold, "trailing dot: {trailing}");
}

// ─── DictionaryDetector ────────────────────────────────────────

fn dict_detector() -> DictionaryDetector {
    let mut m = HashMap::new();
    // Long-form (3+ letter) overlay: stand-in for what the embedded FST holds.
    let en_overlay: HashSet<String> = ["hello", "world", "the", "elm", "wbv-not-a-word"]
        .iter()
        .filter(|s| !s.contains("not-a-word"))
        .map(|s| (*s).to_owned())
        .collect();
    let uk_overlay: HashSet<String> = ["що", "мені", "цим", "привіт", "слово"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    // Short stop words (1-2 letters) — hand-curated per layout.
    let en_stop: HashSet<String> = ["a", "i", "is", "to", "of", "we", "in", "on", "ws-NOT"]
        .iter()
        .filter(|s| !s.contains("NOT"))
        .map(|s| (*s).to_owned())
        .collect();
    let uk_stop: HashSet<String> = ["а", "і", "у", "є", "з", "не", "що", "ці", "ця", "цю"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    m.insert(
        LayoutId::from("en-US"),
        LayoutDictionary::from_overlay_only(en_overlay, en_stop, HashSet::new()),
    );
    m.insert(
        LayoutId::from("uk-UA"),
        LayoutDictionary::from_overlay_only(uk_overlay, uk_stop, HashSet::new()),
    );
    DictionaryDetector::new(m)
}

#[test]
fn dict_keeps_when_current_is_a_word() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "hello".into()), (uk.clone(), "руддщ".into())];
    match dict_detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep, got {other:?}"),
    }
}

#[test]
fn dict_switches_for_punto_full_phrase() {
    // user types "Що мені з цим" while still in en-US — every
    // alt token is a known UK word; every current token is not
    // a known EN word.
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");

    let cases = [("Oj", "Що"), ("vtys", "мені"), ("p", "з"), ("wbv", "цим")];
    for (en_text, uk_text) in cases {
        let cands = vec![(en.clone(), en_text.into()), (uk.clone(), uk_text.into())];
        assert_switches_to(&dict_detector(), &ctx(&en, &cands), &uk);
    }
}

#[test]
fn dict_handles_single_letter_prepositions() {
    // "f" in en (scancode 0x21) → "а" in uk; "f" alone isn't an
    // EN word, "а" is the most common UK preposition.
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "f".into()), (uk.clone(), "а".into())];
    assert_switches_to(&dict_detector(), &ctx(&en, &cands), &uk);
}

/// Regression: 2-letter `ці` (uk-UA, valid) ↔ `ws` (en-US, in the FST
/// as noise). At this length only the curated short-stop lists are
/// trusted, so `ws` claims nothing while `ці` is in `uk_stop` — the
/// user's input is left alone.
#[test]
fn dict_keeps_short_uk_demonstrative() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "ws".into()), (uk.clone(), "ці".into())];
    match dict_detector().judge(&ctx(&uk, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep, got {other:?}"),
    }
}

/// Inverse of the above: same scancodes, but the user is in en-US
/// and `ws` isn't in the curated en stop list, while `ці` IS in
/// the uk stop list — so we *do* switch (matching the user's
/// presumed intent of typing Cyrillic).
#[test]
fn dict_switches_to_short_uk_demonstrative_from_en() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "ws".into()), (uk.clone(), "ці".into())];
    assert_switches_to(&dict_detector(), &ctx(&en, &cands), &uk);
}

/// Regression: 2-letter English acronyms typed under uk-UA render as
/// Cyrillic noise (`AI` → `ФШ`). The detector must switch on the
/// alt-side stop hit, which assumes `ai` is in the en-US short stop
/// list — `build.rs` arranges that by mirroring ≤2-letter entries from
/// `en_us-extras.txt`. This test fakes the arrangement directly.
#[test]
fn dict_switches_short_en_acronym_from_uk_layout() {
    let mut m = HashMap::new();
    // `ai` lives in en-US short stop (the build.rs-mirrored
    // shape). uk-UA stop has the usual prepositions but nothing
    // matching `фш`.
    let en_stop: HashSet<String> = ["a", "i", "ai"].iter().map(|s| (*s).to_owned()).collect();
    let uk_stop: HashSet<String> = ["а", "і", "у", "ні"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    m.insert(
        LayoutId::from("en-US"),
        LayoutDictionary::from_overlay_only(HashSet::new(), en_stop, HashSet::new()),
    );
    m.insert(
        LayoutId::from("uk-UA"),
        LayoutDictionary::from_overlay_only(HashSet::new(), uk_stop, HashSet::new()),
    );
    let det = DictionaryDetector::new(m);

    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "AI".into()), (uk.clone(), "ФШ".into())];
    assert_switches_to(&det, &ctx(&uk, &cands), &en);
}

/// Regression: Hunspell expanded `туча` into every form, including the
/// vocative `туче`, which is also the uk-UA rendering of the en-US
/// scancodes for `next` — so typing `next` under uk-UA landed on a real
/// uk word and was kept. Flagging `туче` weak makes the detector defer
/// to the strong en-US hit.
///
/// A user genuinely typing `туче` with no en-US match must still not be
/// switched to gibberish.
#[test]
fn dict_weak_current_defers_to_strong_alt() {
    // The weak sub-rule lives in Phase 2 of `judge`, so Phase 1
    // (overlay-priority) must not fire: neither word may be in an
    // overlay, hence the FST-baking helper over `from_overlay_only`.
    let mut m = HashMap::new();
    let uk_weak: HashSet<String> = ["туче"].iter().map(|s| (*s).to_owned()).collect();
    m.insert(
        LayoutId::from("en-US"),
        dict_with_embedded(&["next"], HashSet::new()),
    );
    m.insert(
        LayoutId::from("uk-UA"),
        dict_with_embedded_and_weak(&["туче"], HashSet::new(), uk_weak),
    );
    let det = DictionaryDetector::new(m);

    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "next".into()), (uk.clone(), "туче".into())];
    assert_switches_to(&det, &ctx(&uk, &cands), &en);
}

/// Counter-regression: a weak current-side hit must still Keep when no
/// alt is in the dict — the weak list never blocks a switch *by itself*,
/// it only opens the door to one when a strong cross-layout alt exists.
#[test]
fn dict_weak_current_keeps_when_no_alt_in_dict() {
    // No en-US FST entry matches the alt rendering, so the
    // weak-but-no-strong-alt branch must Keep.
    let mut m = HashMap::new();
    let uk_weak: HashSet<String> = ["туче"].iter().map(|s| (*s).to_owned()).collect();
    m.insert(
        LayoutId::from("en-US"),
        dict_with_embedded(&[], HashSet::new()),
    );
    m.insert(
        LayoutId::from("uk-UA"),
        dict_with_embedded_and_weak(&["туче"], HashSet::new(), uk_weak),
    );
    let det = DictionaryDetector::new(m);

    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "qzqz".into()), (uk.clone(), "туче".into())];
    match det.judge(&ctx(&uk, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for weak-current-no-alt, got {other:?}"),
    }
}

#[test]
fn dict_no_opinion_when_neither_is_a_word() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    // Pure noise both ways — punt to the next detector.
    let cands = vec![(en.clone(), "qzxq".into()), (uk.clone(), "ййххй".into())];
    assert_no_opinion(&dict_detector(), &ctx(&en, &cands));
}

#[test]
fn dict_keeps_capitalised_words() {
    // "Hello" with the capital is still in EN dict via lowercase match.
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "Hello".into()), (uk.clone(), "Руддщ".into())];
    match dict_detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep, got {other:?}"),
    }
}

/// Regression: ≥3-letter words on the curated stop list must be
/// honoured by the full-length lookup too. Hunspell has `чути` but not
/// the 1-sg `чую`, and the old `contains` checked only FST + overlay,
/// mis-classifying it as "not a word".
#[test]
fn dict_keeps_long_word_added_to_short_stop_list() {
    // Build a dict whose embedded FST is empty (simulating "this
    // 3-letter word is NOT in the FST"), but whose stop list does
    // contain it.
    let mut m = HashMap::new();
    let en_stop: HashSet<String> = ["a", "i"].iter().map(|s| (*s).to_owned()).collect();
    let uk_stop: HashSet<String> = ["а", "і", "у", "чую"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    m.insert(
        LayoutId::from("en-US"),
        LayoutDictionary::from_overlay_only(HashSet::new(), en_stop, HashSet::new()),
    );
    m.insert(
        LayoutId::from("uk-UA"),
        LayoutDictionary::from_overlay_only(HashSet::new(), uk_stop, HashSet::new()),
    );
    let det = DictionaryDetector::new(m);

    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "xe.".into()), (uk.clone(), "чую".into())];
    match det.judge(&ctx(&uk, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for `чую` from stop list, got {other:?}"),
    }
}

/// Build a [`LayoutDictionary`] with words baked into the embedded
/// FST (not the overlay). Lets the test distinguish "user-supplied
/// signal" from "shipped dictionary" — the whole point of the
/// overlay-priority sweep.
fn dict_with_embedded(embedded_words: &[&str], overlay: HashSet<String>) -> LayoutDictionary {
    dict_with_embedded_and_weak(embedded_words, overlay, HashSet::new())
}

/// Variant of [`dict_with_embedded`] that also seeds the weak
/// list — for tests of the Phase 2 weak-defers-to-strong-alt
/// rule where the same word lives in both the FST (so the dict
/// detector sees it as a real word) and the weak list (so it
/// can be overridden by a cross-layout dict hit).
fn dict_with_embedded_and_weak(
    embedded_words: &[&str],
    overlay: HashSet<String>,
    weak: HashSet<String>,
) -> LayoutDictionary {
    let mut sorted: Vec<String> = embedded_words.iter().map(|s| (*s).to_owned()).collect();
    sorted.sort();
    sorted.dedup();
    let mut builder = fst::SetBuilder::memory();
    for w in &sorted {
        builder.insert(w).expect("FST insert");
    }
    let bytes: Vec<u8> = builder.into_inner().expect("FST finish");
    let set = FstSet::new(bytes.leak() as &'static [u8]).expect("valid FST");
    LayoutDictionary::new(set, overlay, HashSet::new(), weak)
}

/// Regression: a user overlay entry on the *alt* layout must override a
/// coincidental embedded-FST hit on the current one. Adding `будь` to
/// uk-UA extras and typing it under en-US gives `,elm`, which cleans to
/// the real English word `elm` — without overlay priority the engine
/// keeps it and never consults the whitelist.
#[test]
fn dict_overlay_alt_overrides_embedded_current() {
    let mut m = HashMap::new();
    // en-US: `elm` lives in the bundled FST, NOT in the user's
    // overlay (mirrors the real-world state of the embedded
    // English dictionary).
    m.insert(
        LayoutId::from("en-US"),
        dict_with_embedded(&["elm", "hello", "world"], HashSet::new()),
    );
    // uk-UA: user added `будь` to their extras file → it lands
    // in `user_overlay`. Embedded FST is empty here for clarity.
    let uk_overlay: HashSet<String> = ["будь"].iter().map(|s| (*s).to_owned()).collect();
    m.insert(LayoutId::from("uk-UA"), dict_with_embedded(&[], uk_overlay));
    let det = DictionaryDetector::new(m);

    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    // Engine renders the buffer twice: current = `,elm` (cleans
    // to `elm`), alt = `будь`.
    let cands = vec![(en.clone(), ",elm".into()), (uk.clone(), "будь".into())];
    assert_switches_to(&det, &ctx(&en, &cands), &uk);
}

/// Regression: the alt-overlay sweep must not reach past a *clean*
/// dictionary word of the current layout. An undone correction taught
/// the en-US overlay `ghbdsn` — the en-US rendering of uk-UA `привіт` —
/// and from then on every correctly typed `привіт` was rewritten into
/// it. The `,elm` case above still works because its current rendering
/// carries stray punctuation, which is what makes that hit coincidental.
#[test]
fn dict_alt_overlay_never_beats_a_clean_current_word() {
    let mut m = HashMap::new();
    // The poisoned entry, in the user's own en-US overlay.
    let en_overlay: HashSet<String> = ["ghbdsn"].iter().map(|s| (*s).to_owned()).collect();
    m.insert(LayoutId::from("en-US"), dict_with_embedded(&[], en_overlay));
    // The real word, where it belongs: the bundled uk-UA FST.
    m.insert(
        LayoutId::from("uk-UA"),
        dict_with_embedded(&["привіт"], HashSet::new()),
    );
    let det = DictionaryDetector::new(m);

    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (LayoutId::from("en-US"), "ghbdsn".into()),
        (uk.clone(), "привіт".into()),
    ];
    match det.judge(&ctx(&uk, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for a clean current-layout word, got {other:?}"),
    }
}

/// Inverse: user adds the token to the *current* layout's overlay
/// (the `v-strel-zbook` case from the bug report). Strong Keep —
/// no Switch should fire even if the alt rendering also happens
/// to be a word somewhere.
#[test]
fn dict_overlay_current_keeps_over_embedded_alt() {
    let mut m = HashMap::new();
    let en_overlay: HashSet<String> = ["vstrelzbook"].iter().map(|s| (*s).to_owned()).collect();
    m.insert(LayoutId::from("en-US"), dict_with_embedded(&[], en_overlay));
    // Pretend the alt rendering coincidentally hits a UK word
    // in the embedded FST. Current-overlay priority means we
    // still Keep.
    m.insert(
        LayoutId::from("uk-UA"),
        dict_with_embedded(&["млйшащ"], HashSet::new()),
    );
    let det = DictionaryDetector::new(m);

    let en = LayoutId::from("en-US");
    let cands = vec![
        (en.clone(), "v-strel-zbook".into()),
        (LayoutId::from("uk-UA"), "млйшащ".into()),
    ];
    match det.judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep (current overlay), got {other:?}"),
    }
}

// ─── code-token guard ─────────────────────────────────────────

#[test]
fn code_guard_flags_snake_case() {
    assert!(looks_like_code_token("foo_bar"));
    assert!(looks_like_code_token("_private"));
    assert!(looks_like_code_token("trailing_"));
}

#[test]
fn code_guard_flags_camel_and_pascal_case() {
    assert!(looks_like_code_token("getValue"));
    assert!(looks_like_code_token("myFunc"));
    assert!(looks_like_code_token("XMLHttpRequest")); // multiple capitals after lowercase
}

#[test]
fn code_guard_flags_alphanumeric_mix() {
    assert!(looks_like_code_token("var2"));
    assert!(looks_like_code_token("h2o"));
    assert!(looks_like_code_token("addr1"));
}

#[test]
fn code_guard_flags_code_punct() {
    assert!(looks_like_code_token("path\\to"));
    assert!(looks_like_code_token("a;b"));
    assert!(looks_like_code_token("`raw`"));
}

#[test]
fn code_guard_ignores_prose() {
    assert!(!looks_like_code_token("hello"));
    assert!(!looks_like_code_token("Hello"));
    assert!(!looks_like_code_token("привіт"));
    assert!(!looks_like_code_token("Привіт"));
    assert!(!looks_like_code_token("World"));
    assert!(!looks_like_code_token(""));
}

#[test]
fn code_guard_ignores_acronyms() {
    assert!(!looks_like_code_token("URL"));
    assert!(!looks_like_code_token("HTML"));
    assert!(!looks_like_code_token("API"));
}

// ─── acronym guard ─────────────────────────────────────────────

#[test]
fn acronym_guard_flags_short_uppercase() {
    assert!(looks_like_acronym("SQL"));
    assert!(looks_like_acronym("URL"));
    assert!(looks_like_acronym("HTML"));
    assert!(looks_like_acronym("JSON"));
    assert!(looks_like_acronym("HTTPS"));
    // Single letter still uppercase.
    assert!(looks_like_acronym("I"));
}

#[test]
fn acronym_guard_ignores_too_long() {
    // 6+ letters: more likely shouted prose than a deliberate
    // caps acronym, so let the plausibility pipeline decide.
    assert!(!looks_like_acronym("HELLO!"));
    assert!(!looks_like_acronym("ПРИВІТ"));
    assert!(!looks_like_acronym("HEAVENS"));
}

#[test]
fn acronym_guard_ignores_mixed_case() {
    assert!(!looks_like_acronym("Sql"));
    assert!(!looks_like_acronym("sql"));
    assert!(!looks_like_acronym("HtmL"));
    assert!(!looks_like_acronym("Hello"));
    assert!(!looks_like_acronym("Привіт"));
}

#[test]
fn acronym_guard_ignores_empty_and_punctuated() {
    assert!(!looks_like_acronym(""));
    // Punctuation signals "not a clean acronym" — leave to
    // looks_like_code_token / dict.
    assert!(!looks_like_acronym("SQL;"));
    assert!(!looks_like_acronym("h2o"));
    assert!(!looks_like_acronym("API_KEY"));
}

/// Regression: typing `SQL` under en-US would render as `ІЙД`
/// under uk-UA (1 vowel — `і` — vs SQL's 0 vowels). Plausibility
/// scored the alt at ~1.0 vs current 0.25 → switch. Acronym
/// guard now keeps the current as-is.
#[test]
fn plausibility_keeps_short_uppercase_acronym() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![(en.clone(), "SQL".into()), (uk.clone(), "ІЙД".into())];
    match detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for SQL acronym, got {other:?}"),
    }
}

/// Regression: `має` typed under uk-UA was replaced by the German
/// render `vfä`. Its 2/3 vowel ratio sat just outside the old
/// `0.25..=0.55` plateau and scored 0.66, below `keep_threshold`, while
/// `vfä` sat inside it and scored 1.0.
///
/// The plateau widened to `0.25..=0.67`, so `має` scores 1.0 and Keep
/// wins — other alts scoring 1.0 too is irrelevant.
#[test]
fn plausibility_keeps_short_vcv_cyrillic_word() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let ru = LayoutId::from("ru-RU");
    let de = LayoutId::from("de-DE");
    let es = LayoutId::from("es-ES");
    let fr = LayoutId::from("fr-FR");

    let mut profiles = HashMap::new();
    profiles.insert(
        en.clone(),
        LayoutProfile::new(en.clone(), Script::Latin, "aeiouy".chars()),
    );
    profiles.insert(
        uk.clone(),
        LayoutProfile::new(uk.clone(), Script::Cyrillic, "аеиіоуюяєї".chars()),
    );
    profiles.insert(
        ru.clone(),
        LayoutProfile::new(ru.clone(), Script::Cyrillic, "аеёиоуыэюя".chars()),
    );
    profiles.insert(
        de.clone(),
        LayoutProfile::new(de.clone(), Script::Latin, "aeiouäöü".chars()),
    );
    profiles.insert(
        es.clone(),
        LayoutProfile::new(es.clone(), Script::Latin, "aeiouáéíóúü".chars()),
    );
    profiles.insert(
        fr.clone(),
        LayoutProfile::new(fr.clone(), Script::Latin, "aeiouyàâéèêëîïôûùüÿ".chars()),
    );
    let det = WordPlausibilityDetector::new(profiles);

    // Same scancode buffer (`0x2F 0x21 0x28`) rendered through
    // each layout — exact strings the production engine produces.
    let cands = vec![
        (en.clone(), "vf'".into()),
        (uk.clone(), "має".into()),
        (ru.clone(), "маэ".into()),
        (de.clone(), "vfä".into()),
        (es.clone(), "vf´".into()),
        (fr.clone(), "vfù".into()),
    ];
    match det.judge(&ctx(&uk, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!(
            "expected Keep for `має` under uk-UA across the 6-layout candidate set, got {other:?}"
        ),
    }
}

#[test]
fn relative_fit_prefers_real_word() {
    let d = detector();
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    // "hello" rendered through the English layout should fit
    // English at least as well as "руддщ" fits Ukrainian — and
    // the Ukrainian rendering of those scancodes ("руддщ") is
    // a worse fit for Ukrainian than "слово" is.
    let hello_in_en = d.fit(&en, "hello").unwrap();
    let nonsense_in_uk = d.fit(&uk, "руддщ").unwrap();
    let real_uk_word = d.fit(&uk, "слово").unwrap();
    assert!(real_uk_word > nonsense_in_uk);
    assert!(hello_in_en > nonsense_in_uk);
}

// ─── Suggestions (Suggester) ─────────────────────────────────────────

/// Build a surface FST for tests. Mirrors `dict_with_embedded`'s
/// in-memory builder; entries must be `surface_lower`-shaped.
fn surface_set(words: &[&str]) -> FstSet<&'static [u8]> {
    let mut sorted: Vec<String> = words.iter().map(|s| (*s).to_owned()).collect();
    sorted.sort();
    sorted.dedup();
    let mut builder = fst::SetBuilder::memory();
    for w in &sorted {
        builder.insert(w).expect("surface FST insert");
    }
    let bytes: Vec<u8> = builder.into_inner().expect("surface FST finish");
    FstSet::new(bytes.leak() as &'static [u8]).expect("valid surface FST")
}

fn qwerty_geometry() -> KeyboardGeometry {
    let rows: [(u32, &str); 4] = [
        (0x02, "1234567890-="),
        (0x10, "qwertyuiop[]"),
        (0x1E, "asdfghjkl;'"),
        (0x2C, "zxcvbnm,./"),
    ];
    KeyboardGeometry::from_scancode_chars(rows.iter().flat_map(|(base, s)| {
        s.chars()
            .enumerate()
            .map(move |(i, c)| (base + i as u32, c))
    }))
}

fn uk_geometry() -> KeyboardGeometry {
    let rows: [(u32, &str); 3] = [
        (0x10, "йцукенгшщзхї"),
        (0x1E, "фівапролджє"),
        (0x2C, "ячсмитьбю."),
    ];
    KeyboardGeometry::from_scancode_chars(rows.iter().flat_map(|(base, s)| {
        s.chars()
            .enumerate()
            .map(move |(i, c)| (base + i as u32, c))
    }))
}

/// Suggester over a single layout with the given surface corpus,
/// overlay, weak list and keyboard geometry.
fn suggester_for(
    layout: &LayoutId,
    surface_words: &[&str],
    overlay: HashSet<String>,
    weak: HashSet<String>,
    geometry: KeyboardGeometry,
) -> Suggester {
    let dict = LayoutDictionary::from_overlay_only(overlay, HashSet::new(), weak)
        .with_surface(surface_set(surface_words));
    let mut dicts = HashMap::new();
    dicts.insert(layout.clone(), dict);
    let mut geo = HashMap::new();
    geo.insert(layout.clone(), geometry);
    Suggester::new(DictionaryDetector::new(dicts), geo)
}

#[test]
fn suggests_adjacent_key_slip_first() {
    let en = LayoutId::from("en-US");
    // `hwllo`: `w` sits right above/next to `e` — classic slip.
    // `hollow` is also within distance 2 but must rank below.
    let s = suggester_for(
        &en,
        &["hello", "hollow", "hallo"],
        HashSet::new(),
        HashSet::new(),
        qwerty_geometry(),
    );
    let out = s.suggest(&en, "hwllo", 5);
    assert!(!out.is_empty(), "expected suggestions for `hwllo`");
    assert_eq!(out[0].text, "hello");
}

#[test]
fn suggests_transposition() {
    let en = LayoutId::from("en-US");
    let s = suggester_for(
        &en,
        &["hello", "helm"],
        HashSet::new(),
        HashSet::new(),
        qwerty_geometry(),
    );
    let out = s.suggest(&en, "hlelo", 5);
    assert_eq!(out[0].text, "hello");
    assert!(
        out[0].score < 1.0,
        "transposition should cost less than a full edit, got {}",
        out[0].score
    );
}

#[test]
fn restores_apostrophe_from_surface_form() {
    let uk = LayoutId::from("uk-UA");
    // The membership FST stores `пять`; the surface FST stores
    // `п'ять`. The user typed the word without the apostrophe (the
    // uk mapping has no apostrophe key), and the suggestion must
    // come back WITH it — that is the whole reason the surface FST
    // exists.
    let s = suggester_for(
        &uk,
        &["п'ять", "пита"],
        HashSet::new(),
        HashSet::new(),
        uk_geometry(),
    );
    let out = s.suggest(&uk, "пять", 5);
    assert_eq!(out[0].text, "п'ять");
}

#[test]
fn restores_title_case() {
    let uk = LayoutId::from("uk-UA");
    let s = suggester_for(
        &uk,
        &["слово", "слон"],
        HashSet::new(),
        HashSet::new(),
        uk_geometry(),
    );
    // `Слоао`: `а` is one key left of `в` on the uk home row.
    let out = s.suggest(&uk, "Слоао", 5);
    assert_eq!(out[0].text, "Слово");
}

#[test]
fn never_suggests_for_short_tokens() {
    let en = LayoutId::from("en-US");
    let s = suggester_for(
        &en,
        &["abc", "abd"],
        HashSet::new(),
        HashSet::new(),
        qwerty_geometry(),
    );
    assert!(s.suggest(&en, "ab", 5).is_empty());
}

#[test]
fn never_echoes_the_typed_token() {
    let en = LayoutId::from("en-US");
    let s = suggester_for(
        &en,
        &["hello", "hells"],
        HashSet::new(),
        HashSet::new(),
        qwerty_geometry(),
    );
    let out = s.suggest(&en, "hello", 5);
    assert!(out.iter().all(|s| s.text != "hello"));
}

#[test]
fn weak_entries_rank_below_strong_ones() {
    let uk = LayoutId::from("uk-UA");
    let mut weak = HashSet::new();
    weak.insert("хмарі".to_owned());
    // Both candidates are one substitution away from the typo; the
    // weak entry must lose to the everyday word.
    let s = suggester_for(
        &uk,
        &["хмара", "хмарі"],
        HashSet::new(),
        weak,
        uk_geometry(),
    );
    let out = s.suggest(&uk, "хмарв", 5);
    let strong = out.iter().position(|x| x.text == "хмара");
    let weak_pos = out.iter().position(|x| x.text == "хмарі");
    match (strong, weak_pos) {
        (Some(a), Some(b)) => assert!(a < b, "weak entry ranked above strong one"),
        (Some(_), None) => {} // weak fell off the score cap — also fine
        other => panic!("expected the strong candidate present, got {other:?}"),
    }
}

#[test]
fn overlay_words_are_suggestable() {
    let en = LayoutId::from("en-US");
    let mut overlay = HashSet::new();
    overlay.insert("kubectl".to_owned());
    let s = suggester_for(&en, &[], overlay, HashSet::new(), qwerty_geometry());
    let out = s.suggest(&en, "kubectk", 5);
    assert_eq!(out[0].text, "kubectl");
}

#[test]
fn distance_two_is_gated_by_token_length() {
    let en = LayoutId::from("en-US");
    // `abcd` (4 letters) is below the d=2 threshold, and `abcdxy`
    // is only reachable at distance 2 → nothing may be offered.
    let s = suggester_for(
        &en,
        &["abcdxy"],
        HashSet::new(),
        HashSet::new(),
        qwerty_geometry(),
    );
    assert!(s.suggest(&en, "abcd", 5).is_empty());
}

#[test]
fn respects_max_count() {
    let en = LayoutId::from("en-US");
    let s = suggester_for(
        &en,
        &[
            "cast", "cost", "cyst", "case", "case", "most", "mist", "must",
        ],
        HashSet::new(),
        HashSet::new(),
        qwerty_geometry(),
    );
    let out = s.suggest(&en, "csst", 2);
    assert!(out.len() <= 2);
}

#[test]
fn surface_lower_folds_apostrophes_and_keeps_hyphens() {
    assert_eq!(surface_lower("П’ЯТЬ"), "п'ять");
    assert_eq!(surface_lower("імʼя"), "ім'я");
    assert_eq!(surface_lower("а-а-а"), "а-а-а");
    assert_eq!(surface_lower("Don't;"), "don't");
}

// ---- Stray-punctuation (cross-layout artifact) regressions ----
//
// Typing `mañana` with en-US active renders as `ma;ana` (0x27 is `ñ` in
// es-ES, `;` in en-US). Its letters-only skeleton `maana` is an
// embedded en-US entry, and `espa;ol` scored a perfect vowel/script
// fit — each detector had its own way of freezing the correction.

#[test]
fn non_word_char_count_exempts_word_marks() {
    assert_eq!(non_word_char_count("ma;ana"), 1);
    assert_eq!(non_word_char_count("don't"), 0);
    assert_eq!(non_word_char_count("п’ять"), 0);
    assert_eq!(non_word_char_count("а-а-а"), 0);
    assert_eq!(non_word_char_count("var2;"), 2);
}

#[test]
fn dict_stray_punct_skeleton_hit_defers_to_alt() {
    let en = LayoutId::from("en-US");
    let es = LayoutId::from("es-ES");
    let mut m = HashMap::new();
    m.insert(
        en.clone(),
        dict_with_embedded(&["maana", "hello"], HashSet::new()),
    );
    m.insert(es.clone(), dict_with_embedded(&["mañana"], HashSet::new()));
    let d = DictionaryDetector::new(m);
    let cands = vec![(en.clone(), "ma;ana".into()), (es.clone(), "mañana".into())];
    assert_switches_to(&d, &ctx(&en, &cands), &es);
}

#[test]
fn dict_clean_skeleton_still_keeps() {
    // Control: the same embedded entry typed WITHOUT stray
    // punctuation is honoured exactly as before.
    let en = LayoutId::from("en-US");
    let es = LayoutId::from("es-ES");
    let mut m = HashMap::new();
    m.insert(en.clone(), dict_with_embedded(&["maana"], HashSet::new()));
    m.insert(es.clone(), dict_with_embedded(&["mañana"], HashSet::new()));
    let d = DictionaryDetector::new(m);
    let cands = vec![(en.clone(), "maana".into()), (es.clone(), "maana".into())];
    match d.judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for clean `maana`, got {other:?}"),
    }
}

#[test]
fn dict_stray_punct_with_no_alt_hit_keeps() {
    // A stray-carrying token no layout can explain stays as typed.
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let mut m = HashMap::new();
    m.insert(en.clone(), dict_with_embedded(&["maana"], HashSet::new()));
    m.insert(uk.clone(), dict_with_embedded(&["привіт"], HashSet::new()));
    let d = DictionaryDetector::new(m);
    let cands = vec![(en.clone(), "ma;ana".into()), (uk.clone(), "ьфжфтф".into())];
    match d.judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep when no alt explains the token, got {other:?}"),
    }
}

#[test]
fn dict_stray_punct_skeleton_ignores_current_overlay() {
    // Even a user-overlay claim on the skeleton must not keep a
    // stray-carrying render: the overlay entry whitelists `maana`,
    // it says nothing about `ma;ana`.
    let en = LayoutId::from("en-US");
    let es = LayoutId::from("es-ES");
    let en_overlay: HashSet<String> = ["maana"].iter().map(|s| (*s).to_owned()).collect();
    let mut m = HashMap::new();
    m.insert(
        en.clone(),
        LayoutDictionary::from_overlay_only(en_overlay, HashSet::new(), HashSet::new()),
    );
    m.insert(es.clone(), dict_with_embedded(&["mañana"], HashSet::new()));
    let d = DictionaryDetector::new(m);
    let cands = vec![(en.clone(), "ma;ana".into()), (es.clone(), "mañana".into())];
    assert_switches_to(&d, &ctx(&en, &cands), &es);
}

#[test]
fn plausibility_stray_punct_kills_current_fit() {
    let d = detector();
    let en = LayoutId::from("en-US");
    let clean = d.fit(&en, "espaol").unwrap();
    let stray = d.fit(&en, "espa;ol").unwrap();
    assert!(stray < clean);
    assert!(
        stray < d.keep_threshold,
        "a token with `;` inside must not clear the keep veto (fit {stray})"
    );
}

#[test]
fn plausibility_no_keep_veto_for_stray_current() {
    // Regression for the live `espa;ol` case: when no reachable
    // alternate renders it better, the verdict must be NoOpinion —
    // not a "plausibly fits" Keep veto.
    let d = detector();
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "espa;ol".into()),
        (uk.clone(), "уыфжщд".into()),
    ];
    assert_no_opinion(&d, &ctx(&en, &cands));
}

// ─── Inflections of a word the user already taught us ─────────────

/// Adding `деплой` must answer for `деплою`, `деплоїмо`, `деплоїти`
/// too. This is the whole reason the rule exists: an inflected
/// language turns one piece of jargon into a dozen tooltip prompts,
/// and a user who has answered once should not be asked again for
/// every ending.
#[test]
fn overlay_entry_covers_its_own_inflections() {
    let uk = LayoutId::from("uk-UA");
    let overlay: HashSet<String> = ["деплой", "тулбар", "змержу"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let s = suggester_for(&uk, &[], overlay, HashSet::new(), uk_geometry());

    for form in [
        "деплой",
        "деплою",
        "деплоїмо",
        "деплоїти",
        "тулбарі",
        "змержимо",
    ] {
        assert!(
            s.is_known(&uk, form),
            "`{form}` is the same word the user already added"
        );
    }
}

/// …but only the same word. A shared opening is not a shared stem:
/// four characters of `реалм` also open `реальний`, and a rule that
/// swallowed those would silence the tooltip across half the
/// language.
#[test]
fn overlay_entry_does_not_cover_merely_similar_words() {
    let uk = LayoutId::from("uk-UA");
    let overlay: HashSet<String> = ["реалм", "скрол"].iter().map(|s| (*s).to_owned()).collect();
    let s = suggester_for(&uk, &[], overlay, HashSet::new(), uk_geometry());

    for other in ["реальний", "реалізація", "скринька", "документ"]
    {
        assert!(
            !s.is_known(&uk, other),
            "`{other}` is a different word and must still be offered suggestions"
        );
    }
}

/// The rule lives on the suggestion path only. Detection keeps
/// working off exact membership: being lenient there would stop
/// corrections for words the user never taught us, which is a
/// correction silently not happening — far worse than one extra
/// tooltip.
#[test]
fn inflection_coverage_does_not_reach_the_detector() {
    let uk = LayoutId::from("uk-UA");
    let overlay: HashSet<String> = ["деплой"].iter().map(|s| (*s).to_owned()).collect();
    let dict = LayoutDictionary::from_overlay_only(overlay, HashSet::new(), HashSet::new());
    let mut dicts = HashMap::new();
    dicts.insert(uk.clone(), dict);
    let d = DictionaryDetector::new(dicts);

    assert!(d.is_word(&uk, "деплой"), "the exact word is in the overlay");
    assert!(
        !d.is_word(&uk, "деплоїмо"),
        "an inflection is not a dictionary member"
    );
    assert!(
        d.overlay_covers_inflection(&uk, "деплоїмо"),
        "…though the suggestion path can still see the family"
    );
}

// ─── compound guard ────────────────────────────────────────────

#[test]
fn compound_segments_splits_hyphen_and_dot() {
    assert_eq!(
        compound_segments("cqrs-client"),
        Some(vec!["cqrs", "client"])
    );
    assert_eq!(
        compound_segments("api.gateway"),
        Some(vec!["api", "gateway"])
    );
    assert_eq!(compound_segments("a-b.c"), Some(vec!["a", "b", "c"]));
    assert_eq!(compound_segments("hello"), None);
    // Empty segments are punctuation, not structure.
    assert_eq!(compound_segments("-lead"), None);
    assert_eq!(compound_segments("trail-"), None);
    assert_eq!(compound_segments("double--dash"), None);
}

#[test]
fn segment_vouches_skips_stubs_and_artifacts() {
    // `будь-що` under en-US is `,elm-oj`: the comma is a cross-layout
    // artifact and `oj` is too short to speak for the token.
    assert!(!segment_vouches(",elm"));
    assert!(!segment_vouches("oj"));
    assert!(segment_vouches("cqrs"));
    assert!(segment_vouches("client"));
}

#[test]
fn paired_segments_needs_the_same_structure_on_both_sides() {
    assert_eq!(
        paired_segments("cqrs-client", "сйкы-сдшуте"),
        Some(vec![("cqrs", "сйкы"), ("client", "сдшуте")])
    );
    // de-DE puts `ß` on the hyphen key, so a German word typed there and
    // read under en-US splits on one side only. Nothing to compare.
    assert_eq!(paired_segments("fu-ball", "fußball"), None);
    assert_eq!(paired_segments("plain", "плфшт"), None);
    assert_eq!(paired_segments("a-b-c", "а-б"), None);
}

/// Regression: `cqrs-client` typed under en-US with ru-RU loaded.
/// Joined, `cqrsclient` has a six-consonant run and a 0.20 vowel ratio
/// — 0.00 en-US fit — while `сйкы-сдшуте` reads as a plausible 0.75,
/// so the engine "corrected" a perfectly good kebab-case identifier.
#[test]
fn plausibility_keeps_kebab_identifier_with_acronym_head() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "cqrs-client".into()),
        (uk.clone(), "сйкі-сдшуте".into()),
    ];
    match detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for cqrs-client, got {other:?}"),
    }
}

/// The same shape with a dot separator, and with the readable segment
/// first — neither position may matter.
#[test]
fn plausibility_keeps_dotted_identifier_with_acronym_tail() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "client.cqrs".into()),
        (uk.clone(), "сдшуте.сйкі".into()),
    ];
    match detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for client.cqrs, got {other:?}"),
    }
}

/// The guard must not swallow genuinely hyphenated wrong-layout prose.
/// `по-перше` renders as `gj-gthit` under en-US: no segment reads as
/// English, so the correction still fires.
#[test]
fn plausibility_still_switches_hyphenated_wrong_layout_word() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "gj-gthit".into()),
        (uk.clone(), "по-перше".into()),
    ];
    assert_switches_to(&detector(), &ctx(&en, &cands), &uk);
}

/// `все-таки` → `dct-nfrb`: a hyphenated word where no segment reads
/// as English at all. The guard has to stay out of the way.
#[test]
fn plausibility_still_switches_hyphenated_consonant_run() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "dct-nfrb".into()),
        (uk.clone(), "все-таки".into()),
    ];
    assert_switches_to(&detector(), &ctx(&en, &cands), &uk);
}

/// `будь-що` → `,elm-oj`: the only segment that reads as English is the
/// two-letter `oj`, below the vouching floor. Above it the guard would
/// veto — this token must stay eligible for the dictionary detector,
/// which is what actually decides it.
#[test]
fn plausibility_does_not_veto_short_segment_compound() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), ",elm-oj".into()),
        (uk.clone(), "будь-що".into()),
    ];
    if let Verdict::Keep { reason } = detector().judge(&ctx(&en, &cands)) {
        panic!("compound guard vetoed `будь-що`: {reason}");
    }
}

/// The guard is comparative, and this is the case that forced it to be.
/// `інтернет-магазин` renders `synthytn-vfufpby` under en-US, where
/// `synthytn` scores a respectable 0.75 — but `інтернет` scores 1.00 in
/// the layout the switch would move to. A segment that reads *no better*
/// here is no evidence at all.
#[test]
fn plausibility_compound_guard_needs_an_advantage_not_just_a_good_score() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "synthytn-vfufpby".into()),
        (uk.clone(), "інтернет-магазин".into()),
    ];
    assert_switches_to(&detector(), &ctx(&en, &cands), &uk);
}

/// Dictionary side of the same class: a compound segment that is a real
/// word in the *current* layout keeps the token, even though the joined
/// skeleton misses in every dictionary.
#[test]
fn dict_keeps_compound_with_real_word_segment() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let cands = vec![
        (en.clone(), "cqrs-hello".into()),
        (uk.clone(), "сйкі-руддщ".into()),
    ];
    match dict_detector().judge(&ctx(&en, &cands)) {
        Verdict::Keep { .. } => (),
        other => panic!("expected Keep for cqrs-hello, got {other:?}"),
    }
}

/// …but not when the alternate explains the same position. The en-US
/// FST is over-inclusive at three letters, and `где-то` renders as
/// `ult-nj`, whose `ult` is in it. `где` being a real Russian word at
/// the same position is what says which reading is the coincidence.
#[test]
fn dict_compound_segment_defers_to_an_alternate_that_explains_it() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let mut m = HashMap::new();
    m.insert(en.clone(), dict_with_embedded(&["ult"], HashSet::new()));
    // `гдето` is the joined skeleton the alt sweep looks up; `где` is
    // the segment that has to be seen before the guard can defer to it.
    m.insert(
        uk.clone(),
        dict_with_embedded(&["где", "то", "гдето"], HashSet::new()),
    );
    let cands = vec![(en.clone(), "ult-nj".into()), (uk.clone(), "где-то".into())];
    assert_switches_to(&DictionaryDetector::new(m), &ctx(&en, &cands), &uk);
}

/// …and the dictionary guard must not fire on an artifact-carrying
/// segment: `будь-ласка` renders as `,elm-kfcrf`, whose `,elm` cleans
/// down to the real English word `elm` by coincidence alone.
#[test]
fn dict_ignores_compound_segment_carrying_an_artifact() {
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    let mut m = HashMap::new();
    m.insert(en.clone(), dict_with_embedded(&["elm"], HashSet::new()));
    // Dictionaries hold the `letters_only_lower` skeleton, hyphen gone.
    m.insert(
        uk.clone(),
        dict_with_embedded(&["будьласка"], HashSet::new()),
    );
    let cands = vec![
        (en.clone(), ",elm-kfcrf".into()),
        (uk.clone(), "будь-ласка".into()),
    ];
    assert_switches_to(&DictionaryDetector::new(m), &ctx(&en, &cands), &uk);
}
