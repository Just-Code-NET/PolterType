//! The compound guard against the **real** bundled layouts and
//! dictionaries, both directions at once.
//!
//! `poltertype-detect`'s own tests run on toy profiles and hand-built
//! FSTs, which is what let the first version of this guard pass every
//! unit test while vetoing a fifth of a real Russian corpus: segments
//! like `yfitve` (`нашему`) read as perfect English, and only the
//! 370k/1.4M-entry dictionaries and the real vowel profiles show it.
//! Any change to the guard has to answer to both lists below.

// An integration test is its own crate, so `lib.rs`'s `cfg_attr(test,
// …)` relaxation does not reach here.
#![allow(clippy::expect_used)]

use poltertype_core::layouts::LayoutDb;
use poltertype_detect::{
    DetectionContext, Detector, DictionaryDetector, LayoutId, Verdict, WordPlausibilityDetector,
};
use poltertype_types::WordKey;

/// Kebab- and dot-joined tokens a developer types in English. None may
/// be auto-switched — corrupting an identifier is the expensive error.
const IDENTIFIERS: &[&str] = &[
    "cqrs-client",
    "client.cqrs",
    "grpc-server",
    "api-gateway",
    "kubectl-plugin",
    "nginx-ingress",
    "redis-cache",
    "dto-mapper",
    "oauth-token",
    "user-id",
    "well-known",
    "e-mail",
    "x-ray",
    "read-only",
    "up-to-date",
    "cross-platform",
    "open-source",
    "state-of-the-art",
];

/// Hyphenated words typed in the wrong layout. Every one must still be
/// corrected — this is the capability the guard is allowed to cost
/// nothing of.
const UK_HYPHENATED: &[&str] = &[
    "по-перше",
    "по-друге",
    "будь-ласка",
    "будь-що",
    "будь-хто",
    "все-таки",
    "хто-небудь",
    "де-факто",
    "казна-що",
    "хтозна-хто",
    "давним-давно",
    "жовто-блакитний",
    "науково-технічний",
    "інтернет-магазин",
    "бізнес-план",
    "екс-міністр",
];

const RU_HYPHENATED: &[&str] = &[
    "что-то",
    "кто-то",
    "где-то",
    "куда-то",
    "кое-что",
    "кое-где",
    "что-нибудь",
    "где-нибудь",
    "по-моему",
    "по-русски",
    "из-за",
    "из-под",
    "во-первых",
    "давным-давно",
    "интернет-магазин",
    "бизнес-план",
    "горе-руководитель",
    "черно-белый",
];

fn keys_for(db: &LayoutDb, layout: &LayoutId, text: &str) -> Option<Vec<WordKey>> {
    let m = db.get(layout)?;
    text.chars()
        .map(|c| {
            m.key_for_char(c).map(|(scancode, shift)| WordKey {
                scancode,
                shift,
                timestamp_ms: 0,
            })
        })
        .collect()
}

/// Run the shipping detector pair over `keys` with `current` active,
/// exactly as `SwitcherEngine::decide` does — first non-`NoOpinion`
/// verdict wins.
fn verdict(db: &LayoutDb, current: &LayoutId, keys: &[WordKey]) -> Verdict {
    let candidates: Vec<(LayoutId, String)> = db
        .iter()
        .map(|(id, m)| (id.clone(), m.translate_buffer(keys)))
        .collect();
    let ctx = DetectionContext {
        current_layout: current,
        candidates: &candidates,
        recent_context: "",
    };
    let dict = DictionaryDetector::new(
        db.iter()
            .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
            .collect(),
    );
    let plausibility = WordPlausibilityDetector::new(
        db.iter()
            .map(|(id, m)| (id.clone(), m.detector_profile()))
            .collect(),
    );
    for d in [&dict as &dyn Detector, &plausibility as &dyn Detector] {
        match d.judge(&ctx) {
            Verdict::NoOpinion => continue,
            v => return v,
        }
    }
    Verdict::NoOpinion
}

#[test]
fn identifiers_typed_in_english_are_never_switched() {
    let db = LayoutDb::load_embedded();
    let en = LayoutId::from("en-US");
    let mut switched = Vec::new();
    for token in IDENTIFIERS {
        let keys = keys_for(&db, &en, token).expect("en-US maps every ASCII token");
        if let Verdict::Switch(v) = verdict(&db, &en, &keys) {
            switched.push(format!("{token} → {} ({})", v.best_layout, v.reason));
        }
    }
    assert!(
        switched.is_empty(),
        "identifiers were auto-switched:\n  {}",
        switched.join("\n  ")
    );
}

#[test]
fn hyphenated_words_typed_in_the_wrong_layout_are_still_corrected() {
    let db = LayoutDb::load_embedded();
    let en = LayoutId::from("en-US");
    let mut missed = Vec::new();
    for (native, corpus) in [("uk-UA", UK_HYPHENATED), ("ru-RU", RU_HYPHENATED)] {
        let native = LayoutId::from(native);
        for word in corpus {
            let keys = keys_for(&db, &native, word).expect("bundled layout maps its own words");
            // Typed in `native`'s letters but with en-US active — the
            // wrong-layout case the whole app exists for.
            //
            // Any switch away from en-US counts. Every bundled Cyrillic
            // layout is a candidate here, and `что-то` really is also
            // valid Ukrainian and Bulgarian text, so which one wins is a
            // separate question the user's `[languages].active` list and
            // the OS's own answer settle. What must never happen is a
            // `Keep`.
            match verdict(&db, &en, &keys) {
                Verdict::Switch(v) if v.best_layout != en => {}
                other => missed.push(format!("{word} ({native}): {other:?}")),
            }
        }
    }
    assert!(
        missed.is_empty(),
        "wrong-layout corrections lost:\n  {}",
        missed.join("\n  ")
    );
}

/// PolterType's own name, typed on the wrong layout.
///
/// A coined word no general-purpose dictionary carries, so until it was
/// added to `data/wordlists/en_us-extras.txt` the app could not fix the
/// one word every user of it is guaranteed to type.
#[test]
fn the_apps_own_name_is_corrected_from_a_cyrillic_layout() {
    let db = LayoutDb::load_embedded();
    let en = LayoutId::from("en-US");
    // The physical keys of "poltertype" — what reaches the app is
    // whatever the active layout makes of them.
    let keys = keys_for(&db, &en, "poltertype").expect("en-US maps every ASCII token");
    let mut missed = Vec::new();
    for native in ["uk-UA", "ru-RU"] {
        let native = LayoutId::from(native);
        match verdict(&db, &native, &keys) {
            Verdict::Switch(v) if v.best_layout == en => {}
            other => missed.push(format!("{native}: {other:?}")),
        }
    }
    assert!(
        missed.is_empty(),
        "`poltertype` was not corrected to en-US:\n  {}",
        missed.join("\n  ")
    );
}
