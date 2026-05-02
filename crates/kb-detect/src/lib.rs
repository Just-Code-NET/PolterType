//! Language detection pipeline.
//!
//! Pluggable: the engine holds `Vec<Box<dyn Detector>>` and runs them
//! in priority order. The first verdict whose confidence clears the
//! engine's threshold wins.
//!
//! v0.1 ships [`WordPlausibilityDetector`]. Real n-gram / dictionary /
//! ML detectors arrive with the AI subsystem (Phase 7) without any
//! trait change.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fst::Set as FstSet;
pub use kb_types::{DetectionInput, DetectionVerdict, LayoutId};
use serde::{Deserialize, Serialize};

/// A detector judges which keyboard layout the user *intended*.
///
/// Three possible outcomes (see [`Verdict`]):
///
/// * `NoOpinion` — defer to the next detector in the pipeline.
/// * `Keep` — actively veto a switch (used by the dictionary when
///   the current text is already a valid word).
/// * `Switch` — request a layout change.
///
/// The engine runs detectors in priority order and stops at the
/// first non-`NoOpinion`. This is the load-bearing change that lets
/// the dictionary detector say "this is fine, don't ask anyone else"
/// — vital for avoiding false positives on real prose words.
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict;
}

/// What a [`Detector`] decided.
#[derive(Debug, Clone)]
pub enum Verdict {
    /// No opinion — try the next detector.
    NoOpinion,
    /// Strong veto: leave the buffer alone, even if later detectors
    /// would suggest a switch.
    Keep { reason: String },
    /// Switch the active layout to the named one.
    Switch(DetectionVerdict),
}

// ─── WordRewriter (Phase 7+: AI-driven post-correction tricks) ──────

/// A word rewriter operates *after* layout detection: it looks at the
/// final text and may suggest a different one. Used for the
/// "smart-capitalize", "expand-acronym", "slang-to-formal" kind of
/// power-user tricks the AI subsystem (see `kb-ai`) enables.
///
/// Rewriters are off by default; the engine respects the
/// `[ai].rewriters_enabled` flag and the per-rewriter
/// `require_confirmation` toggle.
pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}

#[derive(Debug, Clone, Copy)]
pub struct RewriteRequest<'a> {
    pub original: &'a str,
    pub layout: &'a LayoutId,
    pub recent_context: &'a str,
}

#[derive(Debug, Clone)]
pub enum RewriteVerdict {
    Keep,
    Replace {
        text: String,
        reason: String,
        require_confirmation: bool,
    },
}

/// Engine-supplied context: the buffer already rendered through every
/// candidate layout, so detectors don't depend on layout-mapping types.
#[derive(Debug, Clone)]
pub struct DetectionContext<'a> {
    pub current_layout: &'a LayoutId,
    /// `(layout, text rendered through that layout's mapping)`.
    pub candidates: &'a [(LayoutId, String)],
    pub recent_context: &'a str,
}

impl DetectionContext<'_> {
    pub fn text_for(&self, layout: &LayoutId) -> Option<&str> {
        self.candidates
            .iter()
            .find(|(l, _)| l == layout)
            .map(|(_, t)| t.as_str())
    }
}

// ─── Script enum (paste / cross-script detection) ────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Armenian,
    Hebrew,
    Arabic,
    Other,
}

impl Script {
    pub fn of(c: char) -> Self {
        let cp = c as u32;
        match cp {
            0x0041..=0x005A | 0x0061..=0x007A => Self::Latin,
            0x00C0..=0x024F => Self::Latin,
            0x0400..=0x052F => Self::Cyrillic,
            0x0370..=0x03FF => Self::Greek,
            0x0530..=0x058F => Self::Armenian,
            0x0590..=0x05FF => Self::Hebrew,
            0x0600..=0x06FF => Self::Arabic,
            _ => Self::Other,
        }
    }
}

// ─── Per-layout linguistic data ──────────────────────────────────────

/// Tiny per-layout linguistic profile used by the plausibility scorer.
/// Loaded from the layout-mapping TOMLs in `kb-core::layouts`.
#[derive(Debug, Clone)]
pub struct LayoutProfile {
    pub id: LayoutId,
    pub script: Script,
    /// Lowercase vowel chars for this layout's language. Real words
    /// have characteristic vowel ratios; noise typed through the wrong
    /// layout usually doesn't.
    pub vowels: HashSet<char>,
}

impl LayoutProfile {
    pub fn new(id: LayoutId, script: Script, vowels: impl IntoIterator<Item = char>) -> Self {
        Self {
            id,
            script,
            vowels: vowels.into_iter().collect(),
        }
    }
}

// ─── WordPlausibilityDetector ────────────────────────────────────────

/// Picks the candidate whose text *looks like a word* in its layout's
/// language, using vowel-ratio and consonant-cluster heuristics.
pub struct WordPlausibilityDetector {
    profiles: HashMap<LayoutId, LayoutProfile>,
    pub min_letters: usize,
    pub min_advantage: f32,
}

impl WordPlausibilityDetector {
    pub fn new(profiles: HashMap<LayoutId, LayoutProfile>) -> Self {
        Self {
            profiles,
            min_letters: 3,
            min_advantage: 0.25,
        }
    }

    /// 0.0..=1.0 — higher is "this looks like a real word in `layout`".
    pub fn fit(&self, layout: &LayoutId, text: &str) -> Option<f32> {
        let prof = self.profiles.get(layout)?;
        let letters: Vec<char> = text
            .chars()
            .filter(|c| c.is_alphabetic())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if letters.is_empty() {
            return Some(0.0);
        }

        // (1) Script fit: penalty for letters outside the layout's script.
        let script_hits = letters
            .iter()
            .filter(|&&c| Script::of(c) == prof.script)
            .count();
        let script_fit = script_hits as f32 / letters.len() as f32;

        // (2) Vowel ratio: real words in EN/UK land roughly 0.25..=0.55.
        let vowels = letters.iter().filter(|c| prof.vowels.contains(c)).count();
        let vowel_ratio = vowels as f32 / letters.len() as f32;
        let vowel_fit: f32 = match vowel_ratio {
            r if (0.25..=0.55).contains(&r) => 1.0,
            r => (1.0 - (r - 0.4).abs() * 2.5).clamp(0.0, 1.0),
        };

        // (3) Consonant clusters: count the longest run of non-vowel
        //     letters; words with 4+ consecutive consonants in EN/UK
        //     are extremely rare.
        let mut max_run: u32 = 0;
        let mut run: u32 = 0;
        for c in &letters {
            if prof.vowels.contains(c) {
                if run > max_run {
                    max_run = run;
                }
                run = 0;
            } else {
                run += 1;
            }
        }
        if run > max_run {
            max_run = run;
        }
        let cluster_penalty: f32 = match max_run {
            0..=2 => 0.0,
            3 => 0.25,
            4 => 0.5,
            _ => 0.75,
        };

        Some((script_fit * 0.5 + vowel_fit * 0.5 - cluster_penalty).clamp(0.0, 1.0))
    }
}

impl Detector for WordPlausibilityDetector {
    fn name(&self) -> &'static str {
        "word-plausibility"
    }

    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict {
        // Need a long-enough buffer somewhere to bother deciding.
        let any_long = ctx
            .candidates
            .iter()
            .any(|(_, t)| t.chars().filter(|c| c.is_alphabetic()).count() >= self.min_letters);
        if !any_long {
            return Verdict::NoOpinion;
        }

        let current_text = ctx.text_for(ctx.current_layout).unwrap_or("");
        let current_fit = self.fit(ctx.current_layout, current_text).unwrap_or(0.0);

        let mut best: Option<(&LayoutId, f32)> = None;
        for (layout, text) in ctx.candidates {
            if layout == ctx.current_layout {
                continue;
            }
            let Some(fit) = self.fit(layout, text) else {
                continue;
            };
            if best.is_none_or(|(_, b)| fit > b) {
                best = Some((layout, fit));
            }
        }

        let Some((target, target_fit)) = best else {
            return Verdict::NoOpinion;
        };
        if target_fit - current_fit < self.min_advantage {
            return Verdict::NoOpinion;
        }

        Verdict::Switch(DetectionVerdict {
            best_layout: target.clone(),
            confidence: target_fit,
            reason: format!(
                "plausibility: {target}={:.2}, current {}={:.2}",
                target_fit, ctx.current_layout, current_fit
            ),
        })
    }
}

// ─── DictionaryDetector ──────────────────────────────────────────────

/// Compact immutable per-layout dictionary backed by an FST. Built
/// from the `data/wordlists/<id>.txt` files at compile time (see
/// `kb-core/build.rs`) and optionally augmented at runtime from the
/// user's `<config-dir>/wordlists/<id>.txt` (which is loaded into a
/// supplementary `HashSet`).
///
/// FST is the right structure here:
/// * Encoded byte slice — no per-word allocation, sits in `.rodata`.
/// * O(len(word)) exact lookup; ~1–2 bytes per word stored.
/// * Loads in microseconds; wraps a `&'static [u8]` so we can embed
///   ~370k EN + ~333k UK entries into the binary without bloating
///   resident memory.
#[derive(Clone)]
pub struct LayoutDictionary {
    pub embedded: Arc<FstSet<&'static [u8]>>,
    pub user_overlay: HashSet<String>,
}

impl LayoutDictionary {
    pub fn new(embedded: FstSet<&'static [u8]>, user_overlay: HashSet<String>) -> Self {
        Self {
            embedded: Arc::new(embedded),
            user_overlay,
        }
    }

    /// Convenience: empty embedded FST + given user overlay. Used in
    /// tests where wiring up a real FST would be boilerplate-heavy;
    /// runtime callers always have a real embedded FST.
    ///
    /// Both `expect`s here are infallible by `fst::SetBuilder`'s
    /// contract — building an empty set never errors — but clippy
    /// can't see that, so we silence it locally.
    #[allow(clippy::expect_used)]
    pub fn from_overlay_only(overlay: HashSet<String>) -> Self {
        let empty: Vec<u8> = fst::SetBuilder::memory()
            .into_inner()
            .expect("SetBuilder::memory().into_inner() is infallible");
        let set = FstSet::new(empty.leak() as &'static [u8]).expect("empty FST is always valid");
        Self {
            embedded: Arc::new(set),
            user_overlay: overlay,
        }
    }

    pub fn contains(&self, word_lowercase: &str) -> bool {
        if self.user_overlay.contains(word_lowercase) {
            return true;
        }
        self.embedded.contains(word_lowercase.as_bytes())
    }
}

/// Looks the rendered text up in per-layout dictionaries.
///
/// Decision logic:
///
/// * `current_text` matches its layout's dictionary  → `Keep`
///   (strong signal: the user typed a real word; never switch).
/// * `current_text` is **not** a word AND any alternate IS  →
///   `Switch` to the first matching alternate, confidence ~0.95.
/// * Neither side hits the dictionary  → `NoOpinion` (defer to the
///   plausibility detector).
///
/// Wins:
/// * Catches single-letter prepositions / pronouns (`а`, `і`, `у`,
///   `o`, `a`, `i`) which the plausibility heuristic can't see (its
///   `min_letters` is 3 by design — too noisy below).
/// * Catches *both-look-plausible* tokens (`vtys` ↔ `мені` —
///   plausibility scores them ~equal; the dictionary breaks the tie
///   decisively because `мені` is in the UK dict and `vtys` isn't).
pub struct DictionaryDetector {
    dicts: HashMap<LayoutId, LayoutDictionary>,
}

impl DictionaryDetector {
    pub fn new(dicts: HashMap<LayoutId, LayoutDictionary>) -> Self {
        Self { dicts }
    }

    pub fn is_word(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        self.dicts.get(layout).is_some_and(|d| d.contains(&lower))
    }
}

impl Detector for DictionaryDetector {
    fn name(&self) -> &'static str {
        "dictionary"
    }

    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict {
        let current_text = ctx.text_for(ctx.current_layout).unwrap_or("");

        // Need at least one alphabetic character to even consider it.
        if !current_text.chars().any(|c| c.is_alphabetic()) {
            return Verdict::NoOpinion;
        }

        let current_match = self.is_word(ctx.current_layout, current_text);

        if current_match {
            return Verdict::Keep {
                reason: format!(
                    "current `{current_text}` is a {} dictionary word",
                    ctx.current_layout
                ),
            };
        }

        // Current isn't a known word. Find an alternate that is.
        for (layout, text) in ctx.candidates {
            if layout == ctx.current_layout {
                continue;
            }
            if self.is_word(layout, text) {
                return Verdict::Switch(DetectionVerdict {
                    best_layout: layout.clone(),
                    confidence: 0.95,
                    reason: format!("`{text}` is a {layout} dictionary word"),
                });
            }
        }

        Verdict::NoOpinion
    }
}

// ─── Code-token guard ────────────────────────────────────────────────

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

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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

    fn assert_switches_to(
        detector: &impl Detector,
        ctx: &DetectionContext<'_>,
        expected: &LayoutId,
    ) {
        match detector.judge(ctx) {
            Verdict::Switch(v) => assert_eq!(&v.best_layout, expected),
            other => panic!("expected Switch, got {other:?}"),
        }
    }

    fn assert_no_opinion(detector: &impl Detector, ctx: &DetectionContext<'_>) {
        assert!(matches!(detector.judge(ctx), Verdict::NoOpinion));
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
        assert_no_opinion(&detector(), &ctx(&en, &cands));
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
        let en_overlay: HashSet<String> = ["hello", "world", "the", "is", "a", "i", "to"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let uk_overlay: HashSet<String> =
            ["що", "мені", "з", "цим", "а", "і", "у", "є", "о", "привіт"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
        m.insert(
            LayoutId::from("en-US"),
            LayoutDictionary::from_overlay_only(en_overlay),
        );
        m.insert(
            LayoutId::from("uk-UA"),
            LayoutDictionary::from_overlay_only(uk_overlay),
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
}
