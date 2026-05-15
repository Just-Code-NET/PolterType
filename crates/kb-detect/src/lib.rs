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
use parking_lot::RwLock;
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
    /// If the current text already scores at least this fit value
    /// for its own layout, emit `Verdict::Keep` — don't auto-switch,
    /// even if the alternate scores higher. Defends against false
    /// positives on real-but-uncommon words that aren't in the FST
    /// but read perfectly natural under their layout (think
    /// `kubectl`, `terraform`, `docker`, `nginx`, surnames, …).
    /// Default `0.7` — empirically a good cut between "plausibly a
    /// real word" and "plausibly noise".
    pub keep_threshold: f32,
}

impl WordPlausibilityDetector {
    pub fn new(profiles: HashMap<LayoutId, LayoutProfile>) -> Self {
        Self {
            profiles,
            min_letters: 3,
            min_advantage: 0.25,
            keep_threshold: 0.7,
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

        // (2) Vowel ratio: real words land in a fairly wide band — most
        //     EN / UK / RU prose averages ~0.4 over long text, but
        //     individual short words fan out either way. The plateau
        //     reaches up to 2/3 to cover legitimate V-C-V patterns:
        //     `має` / `оса` / `уса` (Cyrillic) and `eye` / `our` / `ear`
        //     (English) all have vowel-ratio = 0.667 and would
        //     otherwise miss the plateau by a hair, scoring just below
        //     `keep_threshold` and getting auto-switched away —
        //     exactly the regression `має` triggered after the de-DE /
        //     fr-FR layouts joined the candidate set (the German render
        //     `vfä` happens to score 1.0 plausibility because `ä`
        //     lands the vowel-ratio at 1/3 = 0.333). The plateau
        //     centred at 0.46 (midpoint of 0.25 / 0.67) with slope 2.5
        //     matches the previous shape elsewhere — gibberish like
        //     `руддщ` (1 vowel of 5 = 0.2) still falls off as before.
        //     See DECISIONS.md (2026-05-07).
        let vowels = letters.iter().filter(|c| prof.vowels.contains(c)).count();
        let vowel_ratio = vowels as f32 / letters.len() as f32;
        let vowel_fit: f32 = match vowel_ratio {
            r if (0.25..=0.67).contains(&r) => 1.0,
            r => (1.0 - (r - 0.46).abs() * 2.5).clamp(0.0, 1.0),
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

        // Acronym guard: a short all-uppercase token (`SQL`, `URL`,
        // `JSON`) almost always reads as low-vowel "noise" under its
        // own layout while the alt rendering coincidentally lands a
        // vowel and looks Cyrillic-plausible (`SQL` ↔ `ІЙД`). The
        // dict catches the well-known acronyms via the EN extras
        // list; this is the safety net for the long tail. Capped at
        // 5 letters so 6-letter words shouted in caps (`ПРИВІТ`,
        // `HELLO`) still go through the normal scoring path.
        if looks_like_acronym(current_text) {
            return Verdict::Keep {
                reason: format!("current `{current_text}` looks like an all-caps acronym"),
            };
        }

        // Conservative veto: if current already reads as plausible
        // for its layout, don't switch even if the alternate is
        // marginally better. This is what catches `kubectl` /
        // `docker` / surnames that aren't in our dictionary but
        // shouldn't get auto-corrected to Cyrillic noise.
        if current_fit >= self.keep_threshold {
            return Verdict::Keep {
                reason: format!(
                    "current `{current_text}` plausibly fits {} ({:.2} ≥ keep {:.2})",
                    ctx.current_layout, current_fit, self.keep_threshold
                ),
            };
        }

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
    /// Hand-curated 1- and 2-letter stop words. Consulted **instead
    /// of** the full FST when the buffer is ≤ 2 letters long. The
    /// upstream dictionaries we embed are over-inclusive on short
    /// tokens (`dwyl/english-words` ships `ws`, `ax`, `ne`, `oe`,
    /// `ai` as "real" 2-letter words) — relying on them for short
    /// buffers causes aggressive false-positive switches on users
    /// typing legitimate short Cyrillic words.
    pub short_stop_words: HashSet<String>,
}

impl LayoutDictionary {
    pub fn new(
        embedded: FstSet<&'static [u8]>,
        user_overlay: HashSet<String>,
        short_stop_words: HashSet<String>,
    ) -> Self {
        Self {
            embedded: Arc::new(embedded),
            user_overlay,
            short_stop_words,
        }
    }

    /// Convenience: empty embedded FST + given overlay + given short
    /// stop list. Used in tests; runtime callers always have a real
    /// embedded FST.
    ///
    /// Both `expect`s here are infallible by `fst::SetBuilder`'s
    /// contract — building an empty set never errors — but clippy
    /// can't see that, so we silence it locally.
    #[allow(clippy::expect_used)]
    pub fn from_overlay_only(overlay: HashSet<String>, short_stop_words: HashSet<String>) -> Self {
        let empty: Vec<u8> = fst::SetBuilder::memory()
            .into_inner()
            .expect("SetBuilder::memory().into_inner() is infallible");
        let set = FstSet::new(empty.leak() as &'static [u8]).expect("empty FST is always valid");
        Self {
            embedded: Arc::new(set),
            user_overlay: overlay,
            short_stop_words,
        }
    }

    /// Full-dict containment check (used for ≥ 3-letter tokens).
    /// `short_stop_words` is consulted as a fallback so that anything
    /// a user (or maintainer) adds to the curated stop list is
    /// honoured regardless of token length — Hunspell-derived FSTs
    /// often miss inflected / colloquial forms (`чую` from `чути`),
    /// and the stop list is the path-of-least-friction place to
    /// patch them in without regenerating the FST.
    pub fn contains(&self, word_lowercase: &str) -> bool {
        self.contains_in_overlay(word_lowercase)
            || self.embedded.contains(word_lowercase.as_bytes())
    }

    /// Short-token containment check (used for ≤ 2-letter tokens).
    /// Deliberately ignores the FST — it ships too many spurious
    /// 1- and 2-letter "words" (`ws`, `ax`, `oe`, …) that would
    /// trigger false-positive Keep verdicts on short Cyrillic input.
    pub fn contains_short(&self, word_lowercase: &str) -> bool {
        self.contains_in_overlay(word_lowercase)
    }

    /// Overlay-only containment — `user_overlay` + the hand-curated
    /// `short_stop_words` list, both of which are user-influenced
    /// signals (the user's `<stem>-extras.txt` and `<stem>-stop.txt`
    /// files merge into them). Used by [`DictionaryDetector::judge`]
    /// for an overlay-priority sweep so an explicit whitelist entry
    /// like uk-UA `будь` outranks a coincidental embedded-FST hit
    /// on its cross-layout twin (the en-US rendering of `будь` is
    /// `,elm`, which cleans down to the real English word `elm`).
    pub fn contains_in_overlay(&self, word_lowercase: &str) -> bool {
        self.user_overlay.contains(word_lowercase) || self.short_stop_words.contains(word_lowercase)
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
    /// Wrapped in `Arc<RwLock>` so the app can swap dictionaries at
    /// runtime — vital for the "Reload Settings" flow that picks up
    /// user-overlay edits in `<config-dir>/kb-switcher/wordlists/`.
    /// Read locks taken per word lookup; write only on reload.
    /// Contention is negligible.
    dicts: Arc<RwLock<HashMap<LayoutId, LayoutDictionary>>>,
}

impl DictionaryDetector {
    pub fn new(dicts: HashMap<LayoutId, LayoutDictionary>) -> Self {
        Self {
            dicts: Arc::new(RwLock::new(dicts)),
        }
    }

    /// Cheap clone — shares the inner `Arc<RwLock>`. Use this to
    /// hand the app a "reload handle" while the detector itself
    /// lives in the engine's pipeline.
    pub fn handle(&self) -> Self {
        Self {
            dicts: Arc::clone(&self.dicts),
        }
    }

    /// Atomically swap in a fresh dictionary set. The next
    /// `judge`/`is_word`/`is_short_stop_word` call sees the new
    /// data; in-flight reads complete against the old data.
    pub fn replace_dicts(&self, new: HashMap<LayoutId, LayoutDictionary>) {
        *self.dicts.write() = new;
    }

    /// Full-dict word check — used for ≥ 3 letter tokens.
    pub fn is_word(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        let dicts = self.dicts.read();
        dicts.get(layout).is_some_and(|d| d.contains(&lower))
    }

    /// Short-token whitelist check — used for ≤ 2 letter tokens.
    pub fn is_short_stop_word(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        let dicts = self.dicts.read();
        dicts.get(layout).is_some_and(|d| d.contains_short(&lower))
    }

    /// Overlay-only word check (any length). True iff the user added
    /// `text` to this layout's `<stem>.txt` / `<stem>-extras.txt` /
    /// `<stem>-stop.txt` overlay. Drives the overlay-priority sweep
    /// in [`Self::judge`].
    pub fn is_in_overlay(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        let dicts = self.dicts.read();
        dicts
            .get(layout)
            .is_some_and(|d| d.contains_in_overlay(&lower))
    }
}

impl Detector for DictionaryDetector {
    fn name(&self) -> &'static str {
        "dictionary"
    }

    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict {
        let current_raw = ctx.text_for(ctx.current_layout).unwrap_or("");

        // Strip non-letter characters before lookup. Reason: the
        // buffer can legitimately contain a scancode whose current-
        // layout rendering is punctuation but whose alt-layout
        // rendering is a letter (e.g. 0x27 → `;` in en-US, `ж` in
        // uk-UA). The user typing a Cyrillic word in the wrong layout
        // produces a current-render with `;` mid-string; we still
        // want to compare against dictionary entries built from
        // pure-letter words.
        let current_text = letters_only_lower(current_raw);

        let letter_count = current_text.chars().count();
        if letter_count == 0 {
            return Verdict::NoOpinion;
        }

        // Two regimes — see `LayoutDictionary` doc-comment for why:
        //   ≤ 2 letters: trust only the curated short-stop list
        //                (embedded FST is too noisy at this length).
        //   ≥ 3 letters: trust the full FST (+ user overlay).
        let short = letter_count <= 2;

        let lookup = |layout: &LayoutId, text: &str| -> bool {
            if short {
                self.is_short_stop_word(layout, text)
            } else {
                self.is_word(layout, text)
            }
        };
        let label = if short { "short-stop" } else { "dictionary" };

        // Pre-compute alt-layout renderings once; both the
        // overlay-priority sweep and the embedded sweep walk them.
        // Stripping the alt rendering too handles the rare case
        // where a scancode is "letter in current, punct in alt"
        // (e.g. an apostrophe-position key) — without it we'd lose
        // the dictionary hit on the pure-letter substring.
        let alts: Vec<(&LayoutId, String)> = ctx
            .candidates
            .iter()
            .filter(|(l, _)| l != ctx.current_layout)
            .map(|(l, t)| (l, letters_only_lower(t)))
            .filter(|(_, t)| !t.is_empty())
            .collect();

        // Phase 1 — overlay-priority sweep.
        //
        // A user-supplied overlay entry is an explicit signal: the
        // user took the time to whitelist this exact token for this
        // layout, so it should outrank a coincidental embedded match
        // on the cross-layout twin. Without this priority, an entry
        // like uk-UA `будь` is shadowed because its en-US rendering
        // `,elm` cleans down to the real English word `elm` and the
        // current-side Keep short-circuits before alts get scored.
        //
        // Rule: if the current layout's overlay claims the token →
        // Keep. Else if any alt layout's overlay claims it → Switch
        // (override the embedded lookup that would otherwise
        // declare the current layout the winner).
        if self.is_in_overlay(ctx.current_layout, &current_text) {
            return Verdict::Keep {
                reason: format!(
                    "current `{current_text}` is a {} overlay {label} word",
                    ctx.current_layout
                ),
            };
        }
        for (layout, alt_text) in &alts {
            if self.is_in_overlay(layout, alt_text) {
                return Verdict::Switch(DetectionVerdict {
                    best_layout: (*layout).clone(),
                    confidence: 0.95,
                    reason: format!("`{alt_text}` is a {layout} overlay {label} word"),
                });
            }
        }

        // Phase 2 — embedded-dictionary sweep (current behaviour).
        if lookup(ctx.current_layout, &current_text) {
            return Verdict::Keep {
                reason: format!(
                    "current `{current_text}` is a {} {label} word",
                    ctx.current_layout
                ),
            };
        }
        for (layout, alt_text) in &alts {
            if lookup(layout, alt_text) {
                return Verdict::Switch(DetectionVerdict {
                    best_layout: (*layout).clone(),
                    confidence: 0.95,
                    reason: format!("`{alt_text}` is a {layout} {label} word"),
                });
            }
        }

        Verdict::NoOpinion
    }
}

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
            LayoutDictionary::from_overlay_only(en_overlay, en_stop),
        );
        m.insert(
            LayoutId::from("uk-UA"),
            LayoutDictionary::from_overlay_only(uk_overlay, uk_stop),
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
            LayoutDictionary::from_overlay_only(HashSet::new(), en_stop),
        );
        m.insert(
            LayoutId::from("uk-UA"),
            LayoutDictionary::from_overlay_only(HashSet::new(), uk_stop),
        );
        let det = DictionaryDetector::new(m);

        let en = LayoutId::from("en-US");
        let uk = LayoutId::from("uk-UA");
        let cands = vec![(en.clone(), "AI".into()), (uk.clone(), "ФШ".into())];
        assert_switches_to(&det, &ctx(&uk, &cands), &en);
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
            LayoutDictionary::from_overlay_only(HashSet::new(), en_stop),
        );
        m.insert(
            LayoutId::from("uk-UA"),
            LayoutDictionary::from_overlay_only(HashSet::new(), uk_stop),
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
        let mut sorted: Vec<String> = embedded_words.iter().map(|s| (*s).to_owned()).collect();
        sorted.sort();
        sorted.dedup();
        let mut builder = fst::SetBuilder::memory();
        for w in &sorted {
            builder.insert(w).expect("FST insert");
        }
        let bytes: Vec<u8> = builder.into_inner().expect("FST finish");
        let set = FstSet::new(bytes.leak() as &'static [u8]).expect("valid FST");
        LayoutDictionary::new(set, overlay, HashSet::new())
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
}
