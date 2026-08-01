use super::*;

/// Minimal hand-rolled `.aff` covering the key features used by
/// the LibreOffice dictionaries we ingest. Verifies the parser
/// understands FLAG modes, both kinds of conditions, and the
/// `<add>/<continuation>` syntax — without depending on a 6 KB
/// upstream file the tests would have to vendor.
fn aff(s: &str) -> Aff {
    Aff::parse(s).expect("parse")
}

#[test]
fn expands_simple_suffix_with_class_condition() {
    // The exact rule that generates `має` from `мати/Z`.
    let a = aff("SET UTF-8\n\
         SFX Z Y 1\n\
         SFX Z ти є [аіуяї]ти\n");
    let forms = a.expand("мати", "Z");
    assert!(forms.contains("мати"), "stem must be present");
    assert!(forms.contains("має"), "expected `має` in {forms:?}");
}

#[test]
fn expands_unconditional_dot_rule() {
    let a = aff("PFX X Y 1\nPFX X 0 не .\n");
    let forms = a.expand("має", "X");
    assert!(forms.contains("немає"), "expected `немає` in {forms:?}");
}

#[test]
fn negative_class_skips_non_matches() {
    // SFX condition `[^аеи]` — only words ending in NOT a/e/и get
    // the suffix.
    let a = aff("SFX Q Y 1\n\
         SFX Q 0 z [^aei]\n");
    let f1 = a.expand("dog", "Q");
    assert!(f1.contains("dogz"), "[^aei] should match `g`");
    let f2 = a.expand("dia", "Q"); // ends in `a` — class-excluded
    assert_eq!(f2.len(), 1, "no expansion expected, got {f2:?}");
}

#[test]
fn long_flags_chunk_in_pairs() {
    let a = aff("FLAG long\n\
         SFX AB Y 1\n\
         SFX AB 0 s .\n\
         SFX CD Y 1\n\
         SFX CD 0 ed .\n");
    // Flags string `ABCD` = two flags `AB` and `CD`.
    let forms = a.expand("walk", "ABCD");
    assert!(forms.contains("walks"), "expected `walks`");
    assert!(forms.contains("walked"), "expected `walked`");
}

#[test]
fn ignores_unknown_directives() {
    let a = aff("SET UTF-8\n\
         TRY abcde\n\
         MAP 1\n\
         MAP eé\n\
         ICONV ʼ '\n\
         BREAK 1\n\
         BREAK -\n\
         SFX A Y 1\n\
         SFX A 0 s .\n");
    assert!(a.expand("cat", "A").contains("cats"));
}

#[test]
fn continuation_flag_recurses() {
    let a = aff("SFX A Y 1\n\
         SFX A 0 ed/B .\n\
         SFX B Y 1\n\
         SFX B 0 ly .\n");
    let forms = a.expand("walk", "A");
    assert!(forms.contains("walked"), "first-stage SFX A");
    assert!(forms.contains("walkedly"), "B applied to walked");
}

#[test]
fn rule_strips_correctly_in_unicode() {
    // `жити` (live, infinitive) under uk-UA.aff has a Z rule
    // `SFX Z ти веш жити` → `жити` ⇒ `живеш`.
    let a = aff("SFX Z Y 1\nSFX Z ти веш жити\n");
    let forms = a.expand("жити", "Z");
    assert!(forms.contains("живеш"), "got {forms:?}");
}

#[test]
fn num_flags_split_on_commas() {
    // tr_TR's shape: `FLAG num`, one numbered block per surface form,
    // and a `.dic` entry carrying several of them at once.
    let a = aff("FLAG num\n\
         SET UTF-8\n\
         SFX 1 N 1\n\
         SFX 1 0 lar .\n\
         SFX 23 N 1\n\
         SFX 23 0 dan .\n\
         SFX 456 N 1\n\
         SFX 456 0 sız .\n");
    let forms = a.expand("kitap", "1,23,456");
    assert!(forms.contains("kitaplar"), "flag 1 in {forms:?}");
    assert!(forms.contains("kitaptan") || forms.contains("kitapdan"));
    assert!(forms.contains("kitapsız"), "three-digit flag 456");
}

#[test]
fn num_flags_are_not_split_per_digit() {
    // The bug this guards: chunking `"12"` per character would apply
    // flags `1` and `2` instead of the single flag `12`.
    let a = aff("FLAG num\n\
         SFX 1 N 1\n\
         SFX 1 0 a .\n\
         SFX 2 N 1\n\
         SFX 2 0 b .\n\
         SFX 12 N 1\n\
         SFX 12 0 c .\n");
    let forms = a.expand("x", "12");
    assert!(forms.contains("xc"), "flag 12 applies: {forms:?}");
    assert!(!forms.contains("xa"), "flag 1 must NOT apply: {forms:?}");
    assert!(!forms.contains("xb"), "flag 2 must NOT apply: {forms:?}");
}

#[test]
fn num_flags_tolerate_empty_chunks() {
    let a = aff("FLAG num\nSFX 7 N 1\nSFX 7 0 s .\n");
    // A stray trailing / doubled comma must not register a flag named
    // "" that then matches nothing (or, worse, everything).
    let forms = a.expand("cat", "7,,");
    assert!(forms.contains("cats"), "got {forms:?}");
    assert_eq!(forms.len(), 2, "stem + one form only: {forms:?}");
}
