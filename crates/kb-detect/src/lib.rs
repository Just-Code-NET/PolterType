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

pub use kb_types::{DetectionInput, DetectionVerdict, LayoutId};
use serde::{Deserialize, Serialize};

/// A detector judges which keyboard layout the user *intended*.
pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, ctx: &DetectionContext<'_>) -> Option<DetectionVerdict>;
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

    fn detect(&self, ctx: &DetectionContext<'_>) -> Option<DetectionVerdict> {
        // Need a long-enough buffer somewhere to bother deciding.
        let any_long = ctx
            .candidates
            .iter()
            .any(|(_, t)| t.chars().filter(|c| c.is_alphabetic()).count() >= self.min_letters);
        if !any_long {
            return None;
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

        let (target, target_fit) = best?;
        if target_fit - current_fit < self.min_advantage {
            return None;
        }

        Some(DetectionVerdict {
            best_layout: target.clone(),
            confidence: target_fit,
            reason: format!(
                "plausibility: {target}={:.2}, current {}={:.2}",
                target_fit, ctx.current_layout, current_fit
            ),
        })
    }
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

    #[test]
    fn switches_for_typical_punto_case() {
        // user is in uk-UA, typed scancodes for "hello" → uk renders
        // them as "руддщ", en renders them as "hello".
        let en = LayoutId::from("en-US");
        let uk = LayoutId::from("uk-UA");
        let cands = vec![(en.clone(), "hello".into()), (uk.clone(), "руддщ".into())];
        let v = detector().detect(&ctx(&uk, &cands)).expect("should fire");
        assert_eq!(v.best_layout, en);
    }

    #[test]
    fn switches_in_reverse_direction_too() {
        // user in en-US typed scancodes for "привіт" → en renders
        // garbage, uk renders properly.
        let en = LayoutId::from("en-US");
        let uk = LayoutId::from("uk-UA");
        let cands = vec![(en.clone(), "ghbdsn".into()), (uk.clone(), "привіт".into())];
        let v = detector().detect(&ctx(&en, &cands)).expect("should fire");
        assert_eq!(v.best_layout, uk);
    }

    #[test]
    fn keeps_current_when_text_already_native() {
        let en = LayoutId::from("en-US");
        let uk = LayoutId::from("uk-UA");
        let cands = vec![(en.clone(), "hello".into()), (uk.clone(), "руддщ".into())];
        assert!(detector().detect(&ctx(&en, &cands)).is_none());
    }

    #[test]
    fn does_not_switch_for_short_buffer() {
        let en = LayoutId::from("en-US");
        let uk = LayoutId::from("uk-UA");
        let cands = vec![(en.clone(), "ab".into()), (uk.clone(), "фи".into())];
        assert!(detector().detect(&ctx(&en, &cands)).is_none());
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
