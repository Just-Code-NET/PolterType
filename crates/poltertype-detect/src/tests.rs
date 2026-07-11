#![allow(unused_imports)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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

/// Regression: `kubectl` is a real word a developer types in
/// EN but isn't in `dwyl/english-words`. With the old
/// "any-advantage-switches" rule the engine helpfully replaced
/// it with `лгиусед` (UK render of the same scancodes). With
/// `keep_threshold = 0.7` the plausibility detector vetoes the
/// switch because `kubectl` reads perfectly plausibly under en-US.
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
    // `hello` scores ≥ keep_threshold for en-US, so the
    // detector now actively vetoes the switch (Keep) instead
    // of merely abstaining (NoOpinion). Either way the engine
    // doesn't switch — but Keep is the stronger signal.
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

/// Regression: 2-letter `ці` (uk-UA, valid) ↔ `ws` (en-US, accidentally
/// in the FST as a noise word). Old logic switched. New logic only
/// trusts the curated short-stop list at this length, so the fact
/// that `ws` is in the EN FST doesn't matter — it's not in
/// `en_stop`, so neither side claims `Keep` from `ws`, while `ці`
/// IS in `uk_stop` → the engine keeps the user's input alone.
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

/// Regression: 2-letter English acronyms (`AI`, `ML`, `UI`, …)
/// typed under uk-UA render as Cyrillic uppercase noise (`ФШ`,
/// `ЬД`, `ГШ`). The DictionaryDetector must short-Switch on
/// strength of the alt-side stop hit — assuming `ai` lives in
/// the en-US short stop list, which `build.rs` arranges by
/// mirroring ≤2-letter entries from `en_us-extras.txt` into
/// `<dist>/wordlists/en_us-stop.txt`. This test fakes that
/// arrangement by putting `ai` directly in the en stop list.
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

/// Regression: Hunspell expanded `туча` (Ukrainian "thundercloud")
/// into every grammatical form, including the vocative `туче`
/// ("O cloud!") — virtually never typed in modern Ukrainian.
/// That same token is the uk-UA rendering of the en-US scancodes
/// for `next`, so the user typing `next` under uk-UA used to
/// land on `туче`, the dict detector saw a real uk word, and
/// Kept it. With `туче` flagged in the uk weak list (see
/// `data/wordlists/uk_ua-weak.txt`), the detector defers to the
/// strong en-US `next` hit and switches.
#[test]
fn dict_weak_current_defers_to_strong_alt() {
    // Both `next` and `туче` live in their respective EMBEDDED
    // FSTs (mirrors the real-world Hunspell-derived bundled
    // dicts). `туче` is additionally on the uk weak list. The
    // weak sub-rule lives in Phase 2 of `judge`; Phase 1
    // (overlay-priority) must NOT fire here, so neither word is
    // in any overlay — that's why we use the FST-baking helper
    // rather than `from_overlay_only`.
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

/// Counter-regression: a weak current-side hit must still Keep
/// when no alt is in the dict — the weak list never blocks a
/// switch *by itself*, it only opens the door to one when a
/// strong cross-layout alt exists. A user actually typing `туче`
/// in uk-UA (the poet writing about clouds) with no en-US match
/// for the buffer must NOT get auto-switched to gibberish.
#[test]
fn dict_weak_current_keeps_when_no_alt_in_dict() {
    // Same shape as the previous test but the en-US side
    // intentionally has no FST entry that matches the alt
    // rendering — so Phase 2 finds current is weak but no alt
    // is in dict, and the weak-but-no-strong-alt branch must
    // still Keep (the weak list never blocks a switch by
    // itself).
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

/// Regression: ≥3-letter words added to the curated stop list
/// must also be honoured by the full-length lookup path. The
/// Hunspell stems file has `чути` but not the 1-sg `чую`; the
/// stop list is the easy fallback. The old `contains` only
/// checked the FST + user-overlay and would mis-classify `чую`
/// as "not a word" → switch to `xe.` under en-US.
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

/// Regression: a user-supplied overlay entry on the *alt* layout
/// must override a coincidental *embedded*-FST hit on the current
/// layout. The motivating case: user adds `будь` to uk-UA extras,
/// types it while still in en-US (scancodes `,elm`), the
/// detector cleans the current rendering to `elm` — which happens
/// to be a real English word in the embedded FST — and without
/// overlay priority the engine declares "current is English,
/// Keep" and never even consults the user's whitelist.
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

/// Regression (2026-05-07): user types `має` under uk-UA, every
/// candidate set:
///
///   en-US: `vf'`   uk-UA: `має` (current)   ru-RU: `маэ`
///   de-DE: `vfä`   es-ES: `vf´`             fr-FR: `vfù`
///
/// Before the fix: `має` (2/3 vowel ratio = 0.667) sat just outside
/// the old `0.25..=0.55` plateau and scored 0.66, *below* the 0.7
/// `keep_threshold`. The German render `vfä` (1/3 vowel ratio =
/// 0.333) sat *inside* the plateau and scored 1.0 — advantage 0.34
/// over the current → auto-switch fired, deleting the user's
/// Ukrainian word and replacing it with `vfä`.
///
/// After the fix: plateau widened to `0.25..=0.67`, so `має` itself
/// scores 1.0 ≥ keep_threshold → Keep. The fact that German /
/// French alts also score 1.0 is irrelevant — Keep wins.
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
