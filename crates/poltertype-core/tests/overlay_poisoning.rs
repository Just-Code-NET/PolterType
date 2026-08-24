//! A stray overlay entry against the **real** bundled dictionaries.
//!
//! `poltertype-detect`'s own tests run on hand-built FSTs, where the
//! word under test is the only thing in them. What that cannot show is
//! whether a real 333k-entry dictionary actually holds the word the
//! ordering is supposed to defend — and the whole failure this pins was
//! one layout's overlay outranking another layout's shipped dictionary.
//! Both halves have to be the shipping ones for the answer to mean
//! anything.
//!
//! Every pair below was learned into a real user's wordlist by the
//! manual switch-last hotkey, from undoing a correction the engine had
//! got right (see `docs/DECISIONS.md`, 2026-08-21).

// An integration test is its own crate, so `lib.rs`'s `cfg_attr(test,
// …)` relaxation does not reach here.
#![allow(clippy::expect_used)]

use poltertype_core::layouts::LayoutDb;
use poltertype_detect::{
    DetectionContext, Detector, DictionaryDetector, LayoutId, Verdict, WordPlausibilityDetector,
};
use poltertype_types::WordKey;

/// `(the word, the layout it belongs to, the layout whose overlay held
/// its twin)`. The twin itself is not written out — it is derived from
/// the real layout tables below, so a mapping change cannot leave this
/// corpus quietly testing the wrong string.
const POISONED: &[(&str, &str, &str)] = &[
    ("привіт", "uk-UA", "en-US"),
    ("справи", "uk-UA", "en-US"),
    ("tasks", "en-US", "uk-UA"),
    ("tickets", "en-US", "uk-UA"),
];

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

/// The shipping detector pair, with `entry` planted in `poisoned`'s
/// user overlay — exactly what `add_word_to_user_overlay` would have
/// left behind.
fn verdict_with_overlay(
    db: &LayoutDb,
    current: &LayoutId,
    keys: &[WordKey],
    poisoned: &LayoutId,
    entry: &str,
) -> Verdict {
    let candidates: Vec<(LayoutId, String)> = db
        .iter()
        .map(|(id, m)| (id.clone(), m.translate_buffer(keys)))
        .collect();
    let ctx = DetectionContext {
        current_layout: current,
        candidates: &candidates,
        recent_context: "",
    };
    let dicts = db
        .iter()
        .filter_map(|(id, m)| {
            let mut dict = m.dictionary.as_ref()?.clone();
            if id == poisoned {
                dict.user_overlay.insert(entry.to_owned());
            }
            Some((id.clone(), dict))
        })
        .collect();
    let dict = DictionaryDetector::new(dicts);
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
fn a_stray_overlay_entry_cannot_beat_a_real_word() {
    let db = LayoutDb::load_embedded();
    let mut broken = Vec::new();
    for (word, native, poisoned) in POISONED {
        let native = LayoutId::from(*native);
        let poisoned = LayoutId::from(*poisoned);
        let keys = keys_for(&db, &native, word).expect("bundled layout maps its own words");
        let twin = db
            .get(&poisoned)
            .expect("both layouts ship")
            .translate_buffer(&keys);

        match verdict_with_overlay(&db, &native, &keys, &poisoned, &twin) {
            Verdict::Switch(v) if v.best_layout != native => {
                broken.push(format!(
                    "{word} ({native}) → {} ({})",
                    v.best_layout, v.reason
                ));
            }
            _ => {}
        }
    }
    assert!(
        broken.is_empty(),
        "a user-overlay entry rewrote a word its own dictionary holds:\n  {}",
        broken.join("\n  ")
    );
}

/// The counterpart the ordering is not allowed to cost: an overlay
/// entry on the alternate layout must still win when the current
/// rendering is *not* a word of the current layout. That is the case
/// the sweep was written for, and moving it below the current-side
/// check is only correct if this still holds.
#[test]
fn an_overlay_entry_still_corrects_a_word_the_current_layout_lacks() {
    let db = LayoutDb::load_embedded();
    let en = LayoutId::from("en-US");
    let uk = LayoutId::from("uk-UA");
    // Jargon nobody's bundled dictionary carries, in either rendering.
    let jargon = "деплоїмо";
    let keys = keys_for(&db, &uk, jargon).expect("uk-UA maps its own words");
    let typed_under_en = db.get(&en).expect("en-US ships").translate_buffer(&keys);

    let verdict = verdict_with_overlay(&db, &en, &keys, &uk, jargon);
    let switched_to = match &verdict {
        Verdict::Switch(v) => Some(&v.best_layout),
        _ => None,
    };
    assert_eq!(
        switched_to,
        Some(&uk),
        "an explicit uk-UA overlay entry must still claim {typed_under_en}, got {verdict:?}"
    );
}
