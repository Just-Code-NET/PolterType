//! [`WordPlausibilityDetector`] — vowel-ratio / consonant-cluster
//! heuristics over every candidate rendering.

use std::collections::HashMap;

use poltertype_types::{DetectionVerdict, LayoutId, logsafe};

use crate::enums::{Script, Verdict};
use crate::text::{
    compound_segments, looks_like_acronym, non_word_char_count, paired_segments, segment_vouches,
};
use crate::traits::Detector;
use crate::types::{DetectionContext, LayoutProfile};

/// Picks the candidate whose text *looks like a word* in its layout's
/// language, using vowel-ratio and consonant-cluster heuristics.
pub struct WordPlausibilityDetector {
    profiles: HashMap<LayoutId, LayoutProfile>,
    pub min_letters: usize,
    pub min_advantage: f32,
    /// If the current text already scores at least this fit for its own
    /// layout, emit `Verdict::Keep` even when an alternate scores
    /// higher. Defends real-but-uncommon words that are not in the FST
    /// yet read naturally (`kubectl`, `nginx`, surnames). Default `0.7`,
    /// empirically the cut between "plausibly a word" and "noise".
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

    /// 0.0..=1.0 — higher means "this looks like a real word in
    /// `layout`". A dot-separated compound is scored one segment at a
    /// time and takes its *worst* segment; see [`dotted_compound`].
    pub fn fit(&self, layout: &LayoutId, text: &str) -> Option<f32> {
        let prof = self.profiles.get(layout)?;
        if let Some(segments) = dotted_compound(text) {
            return Some(
                segments
                    .map(|seg| Self::score(prof, seg))
                    .fold(1.0f32, f32::min),
            );
        }
        Some(Self::score(prof, text))
    }

    /// Why this compound should stay as typed, or `None`.
    ///
    /// `cqrs-client` scores 0.00 for en-US as one string — the
    /// vowel-less acronym drags the joined letters under every
    /// threshold — while `сйкы-сдшуте` reads as a plausible 0.75. Per
    /// segment the picture inverts: `client` is a perfect 1.00 against
    /// its counterpart's 0.75.
    ///
    /// Two rules keep that from costing real corrections, and the
    /// corpus in `poltertype-core/tests/compound_corpus.rs` is what
    /// forced both:
    ///
    /// * **Better, not merely good.** An absolute "some segment reads
    ///   well here" test vetoed a fifth of a real Russian corpus:
    ///   `по-нашему` renders `gj-yfitve`, and `yfitve` scores 1.00
    ///   under en-US just as `нашему` does under ru-RU.
    /// * **Better than *every* alternate.** Comparing only against the
    ///   winner picks whichever layout happens to score that segment
    ///   worst — with all layouts loaded, `куда-то` lost to a bg-BG
    ///   reading while ru-RU explained it perfectly.
    ///
    /// A layout rendering the segment identically is skipped: switching
    /// to it would leave the text exactly as typed, so it is no rival
    /// reading. Without that, es-ES and de-DE veto-block every Latin
    /// identifier they reproduce character for character.
    fn compound_keeps(&self, ctx: &DetectionContext<'_>, current_text: &str) -> Option<String> {
        let segments = compound_segments(current_text)?;
        for (i, segment) in segments.iter().enumerate() {
            if !segment_vouches(segment) {
                continue;
            }
            let here = self.fit(ctx.current_layout, segment)?;
            let mut rivals = 0usize;
            let best_rival = ctx
                .candidates
                .iter()
                .filter(|(layout, _)| layout != ctx.current_layout)
                .filter_map(|(layout, alt_text)| {
                    let alt = *paired_segments(current_text, alt_text)?.get(i)?;
                    (alt.1 != *segment).then_some((layout, alt.1))
                })
                .filter_map(|(layout, alt)| self.fit(layout, alt))
                .inspect(|_| rivals += 1)
                .fold(f32::NEG_INFINITY, f32::max);
            if rivals > 0 && here - best_rival >= self.min_advantage {
                return Some(format!(
                    "compound {}: segment {} fits {} better than any alternate \
                     ({here:.2} vs {best_rival:.2})",
                    logsafe::redact_word(current_text),
                    logsafe::redact_word(segment),
                    ctx.current_layout,
                ));
            }
        }
        None
    }

    /// The plain word-shape score, with no compound handling.
    fn score(prof: &LayoutProfile, text: &str) -> f32 {
        let letters: Vec<char> = text
            .chars()
            .filter(|c| c.is_alphabetic())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if letters.is_empty() {
            return 0.0;
        }

        // (1) Script fit: penalty for letters outside the layout's script.
        let script_hits = letters
            .iter()
            .filter(|&&c| Script::of(c) == prof.script)
            .count();
        let script_fit = script_hits as f32 / letters.len() as f32;

        // (2) Vowel ratio: real words land in a wide band. The plateau
        //     reaches 2/3 to cover V-C-V patterns — `має`, `оса`, `eye`,
        //     `our` all sit at 0.667 and would otherwise score just
        //     below `keep_threshold` and be switched away. Centred at
        //     0.46 with slope 2.5, so gibberish like `руддщ` (0.2) still
        //     falls off. See DECISIONS.md (2026-05-07).
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

        // (4) Stray punctuation: characters that cannot be part of a
        //     word in any layout (`;`, `]`, digits), apostrophes and
        //     hyphens exempt. In the wrong-layout case these are exactly
        //     the scancodes whose alt rendering *is* a letter, so the
        //     rendering that keeps them as punctuation cannot be the
        //     layout meant. Without this term `espa;ol` scored a perfect
        //     1.0 en-US fit and froze the correction. 0.4 per stray: one
        //     drops a 1.0 fit under `keep_threshold`.
        let stray_penalty = non_word_char_count(text) as f32 * 0.4;

        (script_fit * 0.5 + vowel_fit * 0.5 - cluster_penalty - stray_penalty).clamp(0.0, 1.0)
    }
}

/// Recognise a dot-separated compound — hostname, file name, dotted
/// identifier — and hand back its segments.
///
/// The `.` key is a *letter* in the Cyrillic layouts (0x34 is `ю`), so
/// the buffer keeps a domain together as one token and the two
/// renderings are wildly asymmetric: uk-UA gets clean letters while
/// en-US keeps literal dots and eats two stray penalties. The correctly
/// typed domain scored 0.00 against 0.75 and was "corrected" into
/// gibberish, then switched back on the next prose word.
///
/// Dots in a compound are structure, not the cross-layout artifacts the
/// stray term targets. Scoring each segment and keeping the worst
/// separates the populations: every segment of a real hostname reads as
/// a word, whereas a Cyrillic word that merely contains `ю` (`союз` →
/// `cj.p`) leaves segments that read as nothing.
///
/// `None` — the ordinary path — unless the whole token is dots plus
/// word characters. Any other stray character means a wrong-layout
/// rendering, and an empty segment is not compound structure.
fn dotted_compound(text: &str) -> Option<impl Iterator<Item = &str>> {
    if !text.contains('.') {
        return None;
    }
    if non_word_char_count(text) != text.matches('.').count() {
        return None; // stray punctuation beyond the dots
    }
    if text.split('.').any(str::is_empty) {
        return None;
    }
    Some(text.split('.'))
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

        // Acronym guard: a short all-uppercase token reads as low-vowel
        // noise under its own layout while the alt rendering lands a
        // vowel and looks plausible (`SQL` ↔ `ІЙД`). The dict catches
        // the well-known ones through the EN extras; this is the long
        // tail. Capped at 5 letters so shouted words (`ПРИВІТ`) still
        // score normally.
        if looks_like_acronym(current_text) {
            return Verdict::Keep {
                reason: format!(
                    "current {} looks like an all-caps acronym",
                    logsafe::redact_word(current_text)
                ),
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
                    "current {} plausibly fits {} ({:.2} ≥ keep {:.2})",
                    logsafe::redact_word(current_text),
                    ctx.current_layout,
                    current_fit,
                    self.keep_threshold
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

        if let Some(reason) = self.compound_keeps(ctx, current_text) {
            return Verdict::Keep { reason };
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
