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

    /// Why this compound should stay as typed, or `None`: a segment
    /// that reads *better* here than under any alternate. See
    /// DECISIONS.md (2026-08-20); the corpus in
    /// `poltertype-core/tests/compound_corpus.rs` guards it.
    ///
    /// Two traps, both cheap to re-break:
    ///
    /// * **Better than *every* alternate**, not just the best-fitting
    ///   one — that picks whichever layout scores the segment worst,
    ///   and `куда-то` lost to a bg-BG reading while ru-RU explained it
    ///   perfectly.
    /// * **A layout rendering the segment identically is no rival**:
    ///   switching to it would leave the text exactly as typed. Without
    ///   that, es-ES and de-DE veto every Latin identifier they
    ///   reproduce character for character.
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

        let script_hits = letters
            .iter()
            .filter(|&&c| Script::of(c) == prof.script)
            .count();
        let script_fit = script_hits as f32 / letters.len() as f32;

        // The plateau reaches 2/3 to cover V-C-V patterns: `має`,
        // `оса`, `our` sit at 0.667 and would otherwise fall under
        // `keep_threshold` and be switched away. Centred at 0.46 with
        // slope 2.5, so `руддщ` (0.2) still falls off. See DECISIONS.md
        // (2026-05-07).
        let vowels = letters.iter().filter(|c| prof.vowels.contains(c)).count();
        let vowel_ratio = vowels as f32 / letters.len() as f32;
        let vowel_fit: f32 = match vowel_ratio {
            r if (0.25..=0.67).contains(&r) => 1.0,
            r => (1.0 - (r - 0.46).abs() * 2.5).clamp(0.0, 1.0),
        };

        // Longest run of non-vowel letters: 4+ consecutive consonants
        // are extremely rare in EN/UK.
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

        // A stray is exactly a scancode whose alt rendering *is* a
        // letter, so the rendering that keeps it as punctuation cannot
        // be the layout meant. Without this term `espa;ol` scored a
        // perfect 1.0 en-US fit and froze the correction. 0.4 per
        // stray: one drops a 1.0 fit under `keep_threshold`.
        let stray_penalty = non_word_char_count(text) as f32 * 0.4;

        (script_fit * 0.5 + vowel_fit * 0.5 - cluster_penalty - stray_penalty).clamp(0.0, 1.0)
    }
}

/// Recognise a dot-separated compound — hostname, file name, dotted
/// identifier — and hand back its segments. `None`, the ordinary path,
/// unless the whole token is dots plus word characters: any other stray
/// means a wrong-layout rendering, and an empty segment is not
/// structure.
///
/// The `.` key is a *letter* in the Cyrillic layouts (0x34 is `ю`), so
/// a domain stays one token and the en-US rendering eats two stray
/// penalties — 0.00 against uk-UA's 0.75, and a correctly typed domain
/// was "corrected" into gibberish. Scoring each segment and keeping the
/// worst separates that from a Cyrillic word merely containing `ю`
/// (`союз` → `cj.p`), whose segments read as nothing.
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
        let any_long = ctx
            .candidates
            .iter()
            .any(|(_, t)| t.chars().filter(|c| c.is_alphabetic()).count() >= self.min_letters);
        if !any_long {
            return Verdict::NoOpinion;
        }

        let current_text = ctx.text_for(ctx.current_layout).unwrap_or("");
        let current_fit = self.fit(ctx.current_layout, current_text).unwrap_or(0.0);

        // A short all-uppercase token reads as low-vowel noise under
        // its own layout while the alt rendering lands a vowel and looks
        // plausible (`SQL` ↔ `ІЙД`).
        if looks_like_acronym(current_text) {
            return Verdict::Keep {
                reason: format!(
                    "current {} looks like an all-caps acronym",
                    logsafe::redact_word(current_text)
                ),
            };
        }

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
