//! [`WordPlausibilityDetector`] — vowel-ratio / consonant-cluster
//! heuristics over every candidate rendering.

use std::collections::HashMap;

use poltertype_types::{DetectionVerdict, LayoutId};

use crate::enums::{Script, Verdict};
use crate::text::looks_like_acronym;
use crate::traits::Detector;
use crate::types::{DetectionContext, LayoutProfile};

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
