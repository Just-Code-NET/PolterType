//! Spelling suggestions: fuzzy search over the surface-form FSTs plus a
//! keyboard-aware ranking metric. See [`Suggester::suggest`].
//!
//! Canonicalise the token, stream every dictionary word within
//! Levenshtein distance 1 off the layout's surface FST (distance 2 as a
//! second pass when the first found little and the token is long enough
//! for d=2 to mean something), sweep the small user overlay linearly
//! because its entries are user intent, then re-rank with a *weighted*
//! optimal-string-alignment distance: a physical-neighbour substitution
//! costs less than a random one, a transposition less than two edits,
//! and a matching first letter is rewarded.
//!
//! Pure computation over data the crate already owns — no OS access,
//! and token contents never reach a log.

use std::collections::{HashMap, HashSet};

use fst::{IntoStreamer, Streamer};
use levenshtein_automata::{DFA, Distance, LevenshteinAutomatonBuilder, SINK_STATE};
use poltertype_types::LayoutId;

use crate::dictionary::DictionaryDetector;
use crate::text::{letters_only_lower, surface_lower};
use crate::traits::SuggestionProvider;
use crate::types::Suggestion;

/// Cap on raw candidates pulled off the FST stream before ranking —
/// a d=2 automaton over a short token can match thousands of words,
/// and ranking more than this many never changes the visible top-N.
const MAX_RAW_CANDIDATES: usize = 500;

/// Minimum alphabetic length of a token we suggest for. Below this
/// the candidate space is all noise (`cat` is distance 2 from `a`).
const MIN_TOKEN_LETTERS: usize = 3;

/// Minimum alphabetic length for the distance-2 pass.
const MIN_LETTERS_FOR_D2: usize = 5;

/// Ranking scores above this are dropped even if we found nothing
/// better: suggesting a far-away word is worse than staying quiet.
const MAX_SCORE: f32 = 2.6;

/// Transposition cost (`тичба` → `тиба`… `таби`) — swapped fingers.
const TRANSPOSITION_COST: f32 = 0.6;

/// Penalty when the first letters differ (typos rarely start a word)
/// and a smaller one for the last letter.
const FIRST_CHAR_PENALTY: f32 = 0.3;
const LAST_CHAR_PENALTY: f32 = 0.15;

/// Penalty for words on the curated weak list (archaic vocatives,
/// dead inflections) — valid, but almost never what the user meant.
const WEAK_PENALTY: f32 = 0.6;

/// Reward per shared leading character, capped — breaks ties towards
/// candidates that start the way the user actually typed.
const PREFIX_BONUS: f32 = 0.08;
const PREFIX_BONUS_CAP: usize = 4;

/// Physical position of a key on the standard staggered board, in key
/// units: `(row, column)`. Derived purely from the Win SC Set-1
/// scancode — physical geometry is layout-independent, which is the
/// whole point of using scancodes as the canonical key identity.
fn scancode_grid_pos(sc: u32) -> Option<(f32, f32)> {
    match sc {
        // Digits row `1`..`=`.
        0x02..=0x0D => Some((0.0, (sc - 0x02) as f32)),
        // Top letter row `q`..`]`, staggered half a key right.
        0x10..=0x1B => Some((1.0, (sc - 0x10) as f32 + 0.5)),
        // Home row `a`..`'`.
        0x1E..=0x28 => Some((2.0, (sc - 0x1E) as f32 + 0.75)),
        // ANSI backslash / ISO extra home-row key next to Enter.
        0x2B => Some((2.0, 11.75)),
        // Bottom row `z`..`/`.
        0x2C..=0x35 => Some((3.0, (sc - 0x2C) as f32 + 1.25)),
        // ISO 102nd key (`<>|`, left of Z on European boards).
        0x56 => Some((3.0, 0.25)),
        _ => None,
    }
}

/// Per-layout map from produced character to physical key position —
/// what lets the ranking metric see that `слоао` is one finger-slip
/// (`а` sits next to `в`) away from `слово`.
#[derive(Debug, Default, Clone)]
pub struct KeyboardGeometry {
    pos: HashMap<char, (f32, f32)>,
}

impl KeyboardGeometry {
    /// Build from `(scancode, produced char)` pairs — callers feed
    /// both the plain and the shifted character of every mapped key
    /// (lowercased; ranking runs on canonicalised tokens).
    pub fn from_scancode_chars<I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (u32, char)>,
    {
        let mut pos = HashMap::new();
        for (sc, ch) in pairs {
            if let Some(p) = scancode_grid_pos(sc) {
                for low in ch.to_lowercase() {
                    pos.entry(low).or_insert(p);
                }
            }
        }
        Self { pos }
    }

    /// Squared physical distance between the keys producing `a` and
    /// `b`, in key units. `None` when either char isn't on the grid.
    fn proximity_sq(&self, a: char, b: char) -> Option<f32> {
        let (&(ra, ca), &(rb, cb)) = (self.pos.get(&a)?, self.pos.get(&b)?);
        let dr = ra - rb;
        let dc = ca - cb;
        Some(dr * dr + dc * dc)
    }
}

/// Substitution cost for `a` → `b`: graded by physical key distance,
/// so `hwllo` prefers `hello` (w↔e are direct neighbours) over
/// `hallo` (w↔a are diagonal neighbours). Beyond 1.5 key units it's
/// a full-price substitution — that's not a finger slip any more.
fn substitution_cost(geo: Option<&KeyboardGeometry>, a: char, b: char) -> f32 {
    if a == b {
        return 0.0;
    }
    match geo.and_then(|g| g.proximity_sq(a, b)) {
        Some(d2) if d2 <= 2.25 => 0.3 + 0.1 * d2,
        _ => 1.0,
    }
}

/// Weighted optimal-string-alignment (restricted Damerau-Levenshtein)
/// distance. Costs: exact match 0, neighbour-key substitution
/// [`ADJACENT_SUB_COST`], other substitution 1, adjacent transposition
/// [`TRANSPOSITION_COST`], insertion/deletion 1.
fn weighted_osa(typed: &[char], cand: &[char], geo: Option<&KeyboardGeometry>) -> f32 {
    let n = typed.len();
    let m = cand.len();
    if n == 0 {
        return m as f32;
    }
    if m == 0 {
        return n as f32;
    }

    // Three rolling rows of the DP matrix (previous-previous is needed
    // for the transposition case).
    let mut prev2 = vec![0.0f32; m + 1];
    let mut prev = vec![0.0f32; m + 1];
    let mut cur = vec![0.0f32; m + 1];
    for (j, slot) in prev.iter_mut().enumerate() {
        *slot = j as f32;
    }

    for i in 1..=n {
        cur[0] = i as f32;
        for j in 1..=m {
            let a = typed[i - 1];
            let b = cand[j - 1];
            let sub_cost = substitution_cost(geo, a, b);
            let mut best = (prev[j] + 1.0) // deletion
                .min(cur[j - 1] + 1.0) // insertion
                .min(prev[j - 1] + sub_cost); // substitution / match
            if i > 1 && j > 1 && a == cand[j - 2] && typed[i - 2] == b && a != b {
                best = best.min(prev2[j - 2] + TRANSPOSITION_COST);
            }
            cur[j] = best;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// Adapter: drive an [`fst`] set search with a
/// [`levenshtein_automata`] DFA.
///
/// Why not `fst`'s own `levenshtein` feature: its automaton mismatches
/// multibyte queries entirely — even `слово` within distance 1 of
/// itself streams zero results — which rules it out for a Cyrillic-
/// first product. The tantivy crate handles UTF-8 correctly and
/// counts adjacent transpositions as single edits, which is also the
/// better model of how humans actually mistype.
struct LevAutomaton(DFA);

impl fst::Automaton for LevAutomaton {
    type State = u32;

    fn start(&self) -> u32 {
        self.0.initial_state()
    }

    fn is_match(&self, state: &u32) -> bool {
        matches!(self.0.distance(*state), Distance::Exact(_))
    }

    fn can_match(&self, state: &u32) -> bool {
        *state != SINK_STATE
    }

    fn accept(&self, state: &u32, byte: u8) -> u32 {
        self.0.transition(*state, byte)
    }
}

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

                // User-overlay sweep. Overlay entries are stored in the
                // lossy membership shape, but they are explicit user
                // intent — a project word the user whitelisted should
                // be suggestable even though it lives outside the FST.
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
        // Another form of a word the user has already added counts as
        // known. Without this, every inflection of one piece of
        // jargon costs its own trip through the tooltip — the single
        // loudest complaint this feature produces in an inflected
        // language.
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

        // Restore the typed capitalisation: `Слоао` should offer
        // `Слово`, not `слово`. ALL-CAPS tokens never get here (the
        // engine suppresses them upstream), so first-letter title
        // case is the only shape worth mirroring.
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
