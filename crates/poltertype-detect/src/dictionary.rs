//! Dictionary-backed detection: per-layout FST dictionaries plus
//! the [`DictionaryDetector`] that consults them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use fst::Set as FstSet;
use parking_lot::RwLock;
use poltertype_types::{DetectionVerdict, LayoutId, logsafe};

use crate::enums::Verdict;
use crate::text::{letters_only_lower, non_word_char_count, paired_segments, segment_vouches};
use crate::traits::Detector;
use crate::types::DetectionContext;

/// The empty FST every overlay-only dictionary shares, built once.
///
/// One `expect` run at most once per process instead of two on every
/// construction: `fst`'s builder cannot fail on an empty set, which
/// clippy cannot see. The leak is bounded and deliberate — the leaked
/// buffer *is* the `&'static [u8]` the set borrows.
#[allow(clippy::expect_used)]
static EMPTY_FST: LazyLock<Arc<FstSet<&'static [u8]>>> = LazyLock::new(|| {
    let bytes: Vec<u8> = fst::SetBuilder::memory()
        .into_inner()
        .expect("building an empty FST cannot fail");
    Arc::new(FstSet::new(bytes.leak() as &'static [u8]).expect("an empty FST is a valid FST"))
});

/// Compact immutable per-layout dictionary backed by an FST, built
/// from `data/wordlists/<id>.txt` at compile time and optionally
/// augmented at runtime from the user's own overlay.
///
/// FST rather than a `HashSet`: an encoded byte slice in `.rodata`,
/// no per-word allocation, O(len) lookup, ~1–2 bytes per word — which
/// is what lets ~370k EN + ~333k UK entries ship in the binary without
/// costing resident memory.
#[derive(Clone)]
pub struct LayoutDictionary {
    pub embedded: Arc<FstSet<&'static [u8]>>,
    pub user_overlay: HashSet<String>,
    /// Hand-curated 1- and 2-letter stop words, consulted **instead of**
    /// the full FST for buffers ≤ 2 letters. The upstream dictionaries
    /// are over-inclusive on short tokens (`dwyl/english-words` ships
    /// `ws`, `ax`, `ne`, `oe`, `ai`), which produced aggressive false
    /// switches on legitimate short Cyrillic words.
    pub short_stop_words: HashSet<String>,
    /// Hand-curated "weak" entries: grammatically valid but virtually
    /// never the user's intent — archaic vocatives, dead inflections,
    /// dialectal-only forms. The motivating case is uk-UA `туче`, which
    /// shadows the en-US rendering of `next`.
    ///
    /// A weak current-side hit **defers to** any alt-side dict hit; with
    /// no alt hit it still keeps, because it is valid. Strong entries
    /// are unaffected. Weak never blocks a switch by itself — it only
    /// opens the door to one when a strong alt exists.
    pub weak: HashSet<String>,
    /// Surface-form FST for the suggestions engine: same corpus as
    /// `embedded`, canonicalised with [`crate::surface_lower`] rather
    /// than the lossy [`crate::letters_only_lower`], so apostrophes and
    /// hyphens survive — a suggestion is *typed back* into the user's
    /// text, and `п'ять` must not degrade to `пять`. `None` when the
    /// layout ships no `<stem>-surface.fst`.
    pub surface: Option<Arc<FstSet<&'static [u8]>>>,
}

impl LayoutDictionary {
    pub fn new(
        embedded: FstSet<&'static [u8]>,
        user_overlay: HashSet<String>,
        short_stop_words: HashSet<String>,
        weak: HashSet<String>,
    ) -> Self {
        Self {
            embedded: Arc::new(embedded),
            user_overlay,
            short_stop_words,
            weak,
            surface: None,
        }
    }

    /// Attach the surface-form FST (suggestions corpus). Separate from
    /// [`Self::new`] so the many existing constructors and tests don't
    /// carry a parameter they never use.
    #[must_use]
    pub fn with_surface(mut self, surface: FstSet<&'static [u8]>) -> Self {
        self.surface = Some(Arc::new(surface));
        self
    }

    /// Empty embedded FST + the given overlay, short stop list and weak
    /// list.
    ///
    /// Not test-only: the layout loader builds dictionaries this way for
    /// any language with user overlays but no bundled wordlist. Shares
    /// [`EMPTY_FST`] rather than leaking a fresh one per call — that is
    /// a permanent allocation on every settings reload.
    pub fn from_overlay_only(
        overlay: HashSet<String>,
        short_stop_words: HashSet<String>,
        weak: HashSet<String>,
    ) -> Self {
        Self {
            embedded: Arc::clone(&EMPTY_FST),
            user_overlay: overlay,
            short_stop_words,
            weak,
            surface: None,
        }
    }

    /// True iff `word_lowercase` is on this layout's curated weak
    /// list. See [`LayoutDictionary::weak`] for the semantics.
    pub fn is_weak(&self, word_lowercase: &str) -> bool {
        self.weak.contains(word_lowercase)
    }

    /// Full-dict containment check, for ≥ 3-letter tokens.
    /// `short_stop_words` is consulted as a fallback so curated entries
    /// are honoured at any length — Hunspell-derived FSTs often miss
    /// inflected or colloquial forms (`чую` from `чути`), and the stop
    /// list patches them in without regenerating the FST.
    pub fn contains(&self, word_lowercase: &str) -> bool {
        self.contains_in_overlay(word_lowercase)
            || self.embedded.contains(word_lowercase.as_bytes())
    }

    /// Short-token containment check (≤ 2-letter tokens). Deliberately
    /// ignores the FST — see [`LayoutDictionary::short_stop_words`].
    pub fn contains_short(&self, word_lowercase: &str) -> bool {
        self.contains_in_overlay(word_lowercase)
    }

    /// Overlay-only containment: `user_overlay` + `short_stop_words`,
    /// both user-influenced. [`DictionaryDetector::judge`] sweeps these
    /// first so an explicit whitelist entry outranks a coincidental
    /// embedded hit on its cross-layout twin — uk-UA `будь` renders as
    /// `,elm` in en-US, which cleans down to the real word `elm`.
    pub fn contains_in_overlay(&self, word_lowercase: &str) -> bool {
        self.user_overlay.contains(word_lowercase) || self.short_stop_words.contains(word_lowercase)
    }

    /// True iff some `user_overlay` entry is plausibly the same word in
    /// a different grammatical form — see [`shares_inflection_stem`].
    ///
    /// Walks only the user's own additions: this exists to stop
    /// re-asking about a word they already taught us, and stretching it
    /// over 370k bundled entries would silence the tooltip for half the
    /// language.
    pub fn overlay_covers_inflection(&self, word_lowercase: &str) -> bool {
        self.user_overlay
            .iter()
            .any(|w| shares_inflection_stem(w, word_lowercase))
    }
}

/// Shortest shared prefix that may stand in for a stem. Below this
/// the "same word, other ending" reading stops being credible:
/// `реалм` and `реальний` share exactly four characters and nothing
/// else, which is what sets the floor here.
const STEM_MIN_CHARS: usize = 5;

/// Longest ending either side may carry past the shared prefix.
/// Ukrainian and Russian inflect in 1–4 characters (`тулбар` →
/// `тулбарі`); anything longer is a different word that happens to
/// start the same way.
const INFLECTION_TAIL_MAX: usize = 4;

/// Do two words look like the same word in different grammatical forms?
/// Deliberately a shape rule, not a stemmer: a real stemmer per
/// language is a data set of its own, and this has to work for whatever
/// languages the user added. Being wrong is cheap in one direction
/// only — it can silence a suggestion, never authorise a correction —
/// so the rule lives on the suggestion path alone. Without it, a user
/// who adds `деплой` is asked again about `деплою`, `деплоїмо`,
/// `деплоїти`.
fn shares_inflection_stem(a: &str, b: &str) -> bool {
    let shared = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    if shared < STEM_MIN_CHARS {
        return false;
    }
    a.chars().count() - shared <= INFLECTION_TAIL_MAX
        && b.chars().count() - shared <= INFLECTION_TAIL_MAX
}

/// Looks the rendered text up in per-layout dictionaries.
///
/// * `current_text` is in its layout's dictionary → `Keep` (the user
///   typed a real word; never switch).
/// * `current_text` is not, and some alternate is → `Switch`,
///   confidence ~0.95.
/// * Neither hits → `NoOpinion`, deferring to plausibility.
///
/// It earns its place by catching what plausibility cannot:
/// single-letter prepositions (`а`, `і`, `у`, `o`, `a`, `i`), below
/// that detector's `min_letters`, and both-look-plausible tokens like
/// `vtys` ↔ `мені`, where only dictionary membership breaks the tie.
pub struct DictionaryDetector {
    /// `Arc<RwLock>` so the app can swap dictionaries at runtime, which
    /// is what "Reload Settings" and the profile watcher use. Read lock
    /// per lookup, write only on reload; contention is negligible.
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

    /// True iff `layout`'s user overlay already holds another form of
    /// `text` — see [`LayoutDictionary::overlay_covers_inflection`].
    /// Consulted by the suggestion provider only.
    pub fn overlay_covers_inflection(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        let dicts = self.dicts.read();
        dicts
            .get(layout)
            .is_some_and(|d| d.overlay_covers_inflection(&lower))
    }

    /// True iff `text` is on `layout`'s curated weak list — see
    /// [`LayoutDictionary::weak`].
    pub fn is_weak(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        let dicts = self.dicts.read();
        dicts.get(layout).is_some_and(|d| d.is_weak(&lower))
    }

    /// Insert `word` into `layout`'s user overlay **in place** — the hot
    /// path for the tooltip's "add to dictionary". A from-disk reload
    /// would re-read and re-leak every dictionary blob per added word;
    /// this is one `HashSet` insert under the write lock. Persisting to
    /// the overlay file is the caller's job.
    ///
    /// `false` when the layout has no dictionary loaded or the word
    /// normalises to nothing.
    pub fn add_overlay_word(&self, layout: &LayoutId, word: &str) -> bool {
        let normalized = letters_only_lower(word);
        if normalized.is_empty() {
            return false;
        }
        let mut dicts = self.dicts.write();
        match dicts.get_mut(layout) {
            Some(d) => {
                d.user_overlay.insert(normalized);
                true
            }
            None => false,
        }
    }

    /// `Verdict::Keep` when `current_raw` is a compound with a segment
    /// that is a word in the current layout and that no candidate
    /// layout explains at the same position. See the call site in
    /// [`Detector::judge`] for why both halves are needed.
    fn compound_keep(&self, ctx: &DetectionContext<'_>, current_raw: &str) -> Option<Verdict> {
        let segments = crate::text::compound_segments(current_raw)?;
        for (i, segment) in segments.iter().enumerate() {
            if !segment_vouches(segment) {
                continue;
            }
            let word = letters_only_lower(segment);
            if !self.is_word(ctx.current_layout, &word) {
                continue;
            }
            let explained_elsewhere = ctx.candidates.iter().any(|(layout, alt_raw)| {
                layout != ctx.current_layout
                    && paired_segments(current_raw, alt_raw)
                        .and_then(|pairs| pairs.get(i).map(|(_, alt)| *alt))
                        // A layout rendering this segment identically
                        // explains nothing — switching to it would leave
                        // the text exactly as typed, and es-ES and de-DE
                        // reproduce most Latin tokens verbatim.
                        .filter(|alt| *alt != *segment)
                        .is_some_and(|alt| self.is_word(layout, &letters_only_lower(alt)))
            });
            if !explained_elsewhere {
                return Some(Verdict::Keep {
                    reason: format!(
                        "compound {}: segment {} is a {} dictionary word no alternate explains",
                        logsafe::redact_word(current_raw),
                        logsafe::redact_word(&word),
                        ctx.current_layout
                    ),
                });
            }
        }
        None
    }

    /// Run `f` against `layout`'s dictionary under the read lock; `None`
    /// if it has none loaded. Reaching the surface FST and overlays
    /// *through* the hot-swap handle is what makes profile swaps apply
    /// to suggestions and detection at the same instant.
    pub fn with_dict<R>(
        &self,
        layout: &LayoutId,
        f: impl FnOnce(&LayoutDictionary) -> R,
    ) -> Option<R> {
        let dicts = self.dicts.read();
        dicts.get(layout).map(f)
    }
}

impl Detector for DictionaryDetector {
    fn name(&self) -> &'static str {
        "dictionary"
    }

    fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict {
        let current_raw = ctx.text_for(ctx.current_layout).unwrap_or("");

        let current_text = letters_only_lower(current_raw);

        // A raw render carrying stray punctuation is not the word the
        // user typed, so its letters-only match is coincidence
        // (`ma;ana` → `maana`, which the en-US FST contains). Such a hit
        // must never short-circuit a Keep, though it still wins when no
        // alternate hits either.
        let current_has_stray = non_word_char_count(current_raw) > 0;

        let letter_count = current_text.chars().count();
        if letter_count == 0 {
            return Verdict::NoOpinion;
        }

        // ≤ 2 letters: the curated short-stop list only, the FST being
        // too noisy at that length. See `LayoutDictionary`.
        let short = letter_count <= 2;

        let lookup = |layout: &LayoutId, text: &str| -> bool {
            if short {
                self.is_short_stop_word(layout, text)
            } else {
                self.is_word(layout, text)
            }
        };
        let label = if short { "short-stop" } else { "dictionary" };

        // The alt rendering is stripped too, for the rare scancode that
        // is a letter in the current layout and punctuation in the alt.
        //
        // A stray-carrying alt is dropped outright — the mirror of
        // `current_has_stray`, and for the same reason: nobody means to
        // type punctuation inside a word, so its skeleton hitting the
        // over-inclusive FST is coincidence. uk-UA `тех` renders `nt[`
        // under en-US, whose skeleton `nt` the en-US dictionary holds,
        // and the word was destroyed at confidence 0.95.
        let alts: Vec<(&LayoutId, String)> = ctx
            .candidates
            .iter()
            .filter(|(l, t)| l != ctx.current_layout && non_word_char_count(t) == 0)
            .map(|(l, t)| (l, letters_only_lower(t)))
            .filter(|(_, t)| !t.is_empty())
            .collect();

        // Phase 1 — overlay-priority sweep. An overlay entry is an
        // explicit user signal and outranks a coincidental embedded
        // match on the cross-layout twin: uk-UA `будь` renders as `,elm`
        // in en-US, which cleans to the real English word `elm`.
        if !current_has_stray && self.is_in_overlay(ctx.current_layout, &current_text) {
            return Verdict::Keep {
                reason: format!(
                    "current {} is a {} overlay {label} word",
                    logsafe::redact_word(&current_text),
                    ctx.current_layout
                ),
            };
        }
        // A segment only counts when **no** alternate explains the same
        // position: the en-US FST is over-inclusive at three letters
        // (`ult` is in it), and without that the Russian `где-то` →
        // `ult-nj` stopped being corrected. Runs before the alt sweeps,
        // because an alt hit on a compound's *joined* letters is the
        // weaker claim.
        if let Some(keep) = self.compound_keep(ctx, current_raw) {
            return keep;
        }

        // Phase 2 — embedded-dictionary sweep. A current-side hit
        // flagged `weak` does not short-circuit Keep: walk the alts
        // first and switch if any is in dict. Without it, Hunspell-only
        // forms like uk-UA `туче` shadow the far likelier intent (en-US
        // `next`).
        let current_in_dict = lookup(ctx.current_layout, &current_text);
        let current_is_weak = !short && self.is_weak(ctx.current_layout, &current_text);
        // Ahead of **both** alt sweeps, the overlay one included: a real
        // word of the layout it was typed in outranks anything the other
        // side can show. Ranking an alt overlay entry higher let a single
        // bad one — `ghbdsn`, learned as English from an undone
        // correction — destroy `привіт` permanently and invisibly.
        if current_in_dict && !current_is_weak && !current_has_stray {
            return Verdict::Keep {
                reason: format!(
                    "current {} is a {} {label} word",
                    logsafe::redact_word(&current_text),
                    ctx.current_layout
                ),
            };
        }

        let switch_reason = |alt_text: &str, layout: &LayoutId, overlay: bool| -> String {
            let source = if overlay { "overlay " } else { "" };
            if current_is_weak {
                format!(
                    "current {} is a weak {} {label} word; \
                     alt {} is a strong {layout} {source}hit",
                    logsafe::redact_word(&current_text),
                    ctx.current_layout,
                    logsafe::redact_word(alt_text)
                )
            } else if current_in_dict {
                format!(
                    "current render {} carries stray punctuation — \
                     its skeleton is only a coincidental {} hit; \
                     alt {} is a {layout} {source}{label} word",
                    logsafe::redact_word(current_raw),
                    ctx.current_layout,
                    logsafe::redact_word(alt_text)
                )
            } else {
                format!(
                    "{} is a {layout} {source}{label} word",
                    logsafe::redact_word(alt_text)
                )
            }
        };

        // Overlay ahead of embedded across the alts too, for the reason
        // the current side sweeps its own overlay first.
        for (layout, alt_text) in &alts {
            if self.is_in_overlay(layout, alt_text) {
                return Verdict::Switch(DetectionVerdict {
                    best_layout: (*layout).clone(),
                    confidence: 0.95,
                    reason: switch_reason(alt_text, layout, true),
                });
            }
        }
        for (layout, alt_text) in &alts {
            if lookup(layout, alt_text) {
                return Verdict::Switch(DetectionVerdict {
                    best_layout: (*layout).clone(),
                    confidence: 0.95,
                    reason: switch_reason(alt_text, layout, false),
                });
            }
        }

        // A weak word IS valid, and a stray-carrying token with no
        // better explanation stays as typed. Logged apart so the
        // verdict-trail makes this path obvious.
        if current_in_dict {
            let qualifier = if current_is_weak {
                "weak"
            } else {
                "coincidental"
            };
            return Verdict::Keep {
                reason: format!(
                    "current {} is a {qualifier} {} {label} hit \
                     (no alt-side dict hit to override it)",
                    logsafe::redact_word(&current_text),
                    ctx.current_layout
                ),
            };
        }

        Verdict::NoOpinion
    }
}
