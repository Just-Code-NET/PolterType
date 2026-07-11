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
fn flag_num_is_rejected() {
    let err = Aff::parse("FLAG num\n").expect_err("FLAG num should fail");
    assert!(
        err.to_string().contains("FLAG num"),
        "error mentions FLAG num: {err}"
    );
}
