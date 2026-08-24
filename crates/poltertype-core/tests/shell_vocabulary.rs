//! The shell/toolchain entries of `data/wordlists/en_us-extras.txt`
//! against the **real** bundled layouts and dictionaries.
//!
//! Two things have to hold for every one of them, and neither can be
//! seen from the word alone:
//!
//! * it survives the shipping detector pair — the reason it was added
//!   is that shape scoring reads `tmp` as noise and `еьз` as a word;
//! * it does not shadow a real word of another bundled layout. A
//!   dictionary entry is a permanent veto on correcting whatever the
//!   same keys spell elsewhere, which is how one learned twin once
//!   destroyed `привіт` (see `docs/DECISIONS.md`, 2026-08-21).

// An integration test is its own crate, so `lib.rs`'s `cfg_attr(test,
// …)` relaxation does not reach here.
#![allow(clippy::expect_used)]

use poltertype_core::layouts::LayoutDb;
use poltertype_detect::{
    DetectionContext, Detector, DictionaryDetector, LayoutId, Verdict, WordPlausibilityDetector,
    letters_only_lower,
};
use poltertype_types::WordKey;

const EXTRAS: &str = include_str!("../../../data/wordlists/en_us-extras.txt");

/// The section header this corpus is taken from.
const SECTION: &str = "Shell, filesystem and toolchain";

/// Entries listed under [`SECTION`] in the extras file, so adding one
/// there is enough to put it under test.
fn corpus() -> Vec<&'static str> {
    let mut inside = false;
    let mut words = Vec::new();
    for line in EXTRAS.lines() {
        let line = line.trim();
        if line.starts_with("# ----") {
            inside = line.contains(SECTION);
            continue;
        }
        if inside && !line.is_empty() && !line.starts_with('#') {
            words.push(line);
        }
    }
    assert!(
        !words.is_empty(),
        "no entries under `{SECTION}` — was the section renamed?"
    );
    words
}

fn keys_for(db: &LayoutDb, layout: &LayoutId, text: &str) -> Option<Vec<WordKey>> {
    let m = db.get(layout)?;
    text.chars()
        .map(|c| {
            m.key_for_char(c).map(|(scancode, shift)| WordKey {
                scancode,
                shift,
                caps: false,
                timestamp_ms: 0,
            })
        })
        .collect()
}

fn dictionaries(db: &LayoutDb) -> DictionaryDetector {
    DictionaryDetector::new(
        db.iter()
            .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
            .collect(),
    )
}

#[test]
fn shell_vocabulary_is_never_auto_switched() {
    let db = LayoutDb::load_embedded();
    let en = LayoutId::from("en-US");
    let plausibility = WordPlausibilityDetector::new(
        db.iter()
            .map(|(id, m)| (id.clone(), m.detector_profile()))
            .collect(),
    );
    let dict = dictionaries(&db);
    let mut switched = Vec::new();
    for word in corpus() {
        let keys = keys_for(&db, &en, word).expect("en-US maps every ASCII entry");
        let candidates: Vec<(LayoutId, String)> = db
            .iter()
            .map(|(id, m)| (id.clone(), m.translate_buffer(&keys)))
            .collect();
        let ctx = DetectionContext {
            current_layout: &en,
            candidates: &candidates,
            recent_context: "",
        };
        for d in [&dict as &dyn Detector, &plausibility as &dyn Detector] {
            match d.judge(&ctx) {
                Verdict::NoOpinion => continue,
                Verdict::Switch(v) => {
                    switched.push(format!("{word} → {} ({})", v.best_layout, v.reason));
                    break;
                }
                Verdict::Keep { .. } => break,
            }
        }
    }
    assert!(
        switched.is_empty(),
        "shell vocabulary was auto-switched:\n  {}",
        switched.join("\n  ")
    );
}

#[test]
fn shell_vocabulary_shadows_no_other_layouts_word() {
    let db = LayoutDb::load_embedded();
    let en = LayoutId::from("en-US");
    let dict = dictionaries(&db);
    let mut shadowed = Vec::new();
    for word in corpus() {
        let keys = keys_for(&db, &en, word).expect("en-US maps every ASCII entry");
        for (id, mapping) in db.iter() {
            if id == &en {
                continue;
            }
            let alt = mapping.translate_buffer(&keys);
            // A layout that reproduces the entry character for
            // character explains nothing: switching to it would leave
            // the text exactly as typed.
            if alt == word {
                continue;
            }
            let cleaned = letters_only_lower(&alt);
            let hit = if cleaned.chars().count() <= 2 {
                dict.is_short_stop_word(id, &cleaned)
            } else {
                dict.is_word(id, &cleaned)
            };
            if hit {
                shadowed.push(format!("{word} shadows {id} `{alt}`"));
            }
        }
    }
    assert!(
        shadowed.is_empty(),
        "these entries cost another layout a real word:\n  {}",
        shadowed.join("\n  ")
    );
}
