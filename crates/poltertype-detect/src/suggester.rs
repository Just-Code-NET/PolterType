//! Spelling suggestions: fuzzy search over the surface-form FSTs plus a
//! keyboard-aware ranking metric. See [`Suggester::suggest`].
//!
//! Candidates come off the FST by Levenshtein distance ([`crate::lev_automaton`]),
//! then re-rank by [`crate::distance::weighted_osa`].
//!
//! Pure computation over data the crate already owns — no OS access,
//! and token contents never reach a log.

use std::collections::{HashMap, HashSet};

use fst::{IntoStreamer, Streamer};
use levenshtein_automata::LevenshteinAutomatonBuilder;
use poltertype_types::LayoutId;

use crate::consts::{
    FIRST_CHAR_PENALTY, LAST_CHAR_PENALTY, MAX_RAW_CANDIDATES, MAX_SCORE, MIN_LETTERS_FOR_D2,
    MIN_TOKEN_LETTERS, PREFIX_BONUS, PREFIX_BONUS_CAP, WEAK_PENALTY,
};
use crate::dictionary_detector::DictionaryDetector;
use crate::distance::weighted_osa;
use crate::geometry::KeyboardGeometry;
use crate::lev_automaton::LevAutomaton;
use crate::text::{letters_only_lower, surface_lower};
use crate::traits::SuggestionProvider;
use crate::types::Suggestion;

/// Fuzzy-search suggestions provider over the hot-swappable dictionary
/// set. Shares the [`DictionaryDetector`]'s inner state via a handle
/// clone, so per-app wordlist-profile swaps apply to suggestions the
/// same instant they apply to detection.
pub struct Suggester {
    dicts: DictionaryDetector,
    geometry: HashMap<LayoutId, KeyboardGeometry>,
    /// Parametric Levenshtein tables for d=1 / d=2, with transposition
    /// support. Building these is the expensive step (milliseconds);
    /// compiling a per-query DFA from them is linear in the query.
    lev1: LevenshteinAutomatonBuilder,
    lev2: LevenshteinAutomatonBuilder,
}

impl Suggester {
    pub fn new(dicts: DictionaryDetector, geometry: HashMap<LayoutId, KeyboardGeometry>) -> Self {
        Self {
            dicts,
            geometry,
            lev1: LevenshteinAutomatonBuilder::new(1, true),
            lev2: LevenshteinAutomatonBuilder::new(2, true),
        }
    }

    /// Collect raw candidate words for `typed` from `layout`'s surface
    /// FST (distance 1, then optionally 2) and user overlay.
    /// Returned strings are canonicalised surface forms; the bool
    /// marks weak-list entries.
    fn raw_candidates(&self, layout: &LayoutId, typed: &str) -> Vec<(String, bool)> {
        let typed_letters = typed.chars().filter(|c| c.is_alphabetic()).count();
        let typed_stripped = letters_only_lower(typed);
        self.dicts
            .with_dict(layout, |dict| {
                let mut seen: HashSet<String> = HashSet::new();
                let mut out: Vec<(String, bool)> = Vec::new();

                if let Some(surface) = dict.surface.as_ref() {
                    for (pass, builder) in [&self.lev1, &self.lev2].into_iter().enumerate() {
                        let is_d2 = pass == 1;
                        if is_d2 && (typed_letters < MIN_LETTERS_FOR_D2 || out.len() >= 12) {
                            break;
                        }
                        let dfa = LevAutomaton(builder.build_dfa(typed));
                        let mut stream = surface.search(dfa).into_stream();
                        while let Some(key) = stream.next() {
                            if out.len() >= MAX_RAW_CANDIDATES {
                                break;
                            }
                            let Ok(word) = std::str::from_utf8(key) else {
                                continue;
                            };
                            if word == typed || seen.contains(word) {
                                continue;
                            }
                            seen.insert(word.to_owned());
                            let weak = dict.weak.contains(&letters_only_lower(word));
                            out.push((word.to_owned(), weak));
                        }
                    }
                }

                // Overlay entries are stored in the lossy membership
                // shape, but they are explicit user intent: a
                // whitelisted project word must be suggestable even
                // though it lives outside the FST.
                for word in &dict.user_overlay {
                    let len = word.chars().count();
                    if len < MIN_TOKEN_LETTERS
                        || len.abs_diff(typed_letters) > 2
                        || *word == typed_stripped
                        || seen.contains(word.as_str())
                    {
                        continue;
                    }
                    seen.insert(word.clone());
                    out.push((word.clone(), false));
                }
                out
            })
            .unwrap_or_default()
    }
}

impl SuggestionProvider for Suggester {
    fn is_known(&self, layout: &LayoutId, typed_rendering: &str) -> bool {
        let stripped = letters_only_lower(typed_rendering);
        if stripped.chars().count() <= 2 {
            // Short tokens are outside the suggestions regime
            // entirely — report "known" so the engine stays quiet.
            return true;
        }
        if self.dicts.is_word(layout, &stripped) {
            return true;
        }
        // Another form of a word the user already added counts as
        // known: otherwise every inflection of one piece of jargon costs
        // its own trip through the tooltip.
        self.dicts.overlay_covers_inflection(layout, &stripped)
    }

    fn suggest(&self, layout: &LayoutId, typed_rendering: &str, max: usize) -> Vec<Suggestion> {
        if max == 0 {
            return Vec::new();
        }
        let typed = surface_lower(typed_rendering);
        if typed.chars().filter(|c| c.is_alphabetic()).count() < MIN_TOKEN_LETTERS {
            return Vec::new();
        }
        let typed_chars: Vec<char> = typed.chars().collect();
        let geo = self.geometry.get(layout);

        let mut scored: Vec<Suggestion> = self
            .raw_candidates(layout, &typed)
            .into_iter()
            .filter_map(|(cand, weak)| {
                let cand_chars: Vec<char> = cand.chars().collect();
                let mut score = weighted_osa(&typed_chars, &cand_chars, geo);
                if typed_chars.first() != cand_chars.first() {
                    score += FIRST_CHAR_PENALTY;
                }
                if typed_chars.last() != cand_chars.last() {
                    score += LAST_CHAR_PENALTY;
                }
                if weak {
                    score += WEAK_PENALTY;
                }
                let prefix = typed_chars
                    .iter()
                    .zip(&cand_chars)
                    .take_while(|(a, b)| a == b)
                    .count();
                score -= PREFIX_BONUS * prefix.min(PREFIX_BONUS_CAP) as f32;
                (score <= MAX_SCORE).then_some(Suggestion { text: cand, score })
            })
            .collect();

        // Deterministic order: score, then closest length, then bytes.
        scored.sort_by(|a, b| {
            a.score
                .total_cmp(&b.score)
                .then_with(|| {
                    let la = a.text.chars().count().abs_diff(typed_chars.len());
                    let lb = b.text.chars().count().abs_diff(typed_chars.len());
                    la.cmp(&lb)
                })
                .then_with(|| a.text.cmp(&b.text))
        });
        scored.truncate(max);

        // Restore the typed capitalisation. ALL-CAPS tokens never get
        // here (the engine suppresses them upstream), so first-letter
        // title case is the only shape worth mirroring.
        if typed_rendering
            .chars()
            .find(|c| c.is_alphabetic())
            .is_some_and(char::is_uppercase)
        {
            for s in &mut scored {
                s.text = title_case(&s.text);
            }
        }
        scored
    }
}

/// Uppercase the first alphabetic character of `word`.
fn title_case(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut done = false;
    for ch in word.chars() {
        if !done && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            done = true;
        } else {
            out.push(ch);
        }
    }
    out
}
