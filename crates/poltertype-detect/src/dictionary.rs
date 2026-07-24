//! Dictionary-backed detection: per-layout FST dictionaries plus
//! the [`DictionaryDetector`] that consults them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fst::Set as FstSet;
use parking_lot::RwLock;
use poltertype_types::{DetectionVerdict, LayoutId};

use crate::enums::Verdict;
use crate::text::letters_only_lower;
use crate::traits::Detector;
use crate::types::DetectionContext;

/// Compact immutable per-layout dictionary backed by an FST. Built
/// from the `data/wordlists/<id>.txt` files at compile time (see
/// `poltertype-core/build.rs`) and optionally augmented at runtime from the
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
    /// Hand-curated "weak" dictionary entries — words that are
    /// grammatically valid (Hunspell expanded the bulk dict to
    /// include them) but virtually never the user's intent in
    /// modern usage: archaic vocatives, dead inflections,
    /// dialectal-only forms. The motivating case is uk-UA `туче`
    /// (vocative of `туча` — "O cloud!"), which shadows the en-US
    /// rendering of `next` and used to leave the engine no signal
    /// to switch.
    ///
    /// Effect on the dictionary detector: a weak current-side hit
    /// **defers to** any alt-side dict hit. If no alt is in dict,
    /// the weak entry still keeps (it IS valid, after all). Strong
    /// (non-weak) entries are unaffected — they continue to win
    /// outright.
    ///
    /// Weak is the per-layout asymmetric counterpart of the
    /// existing per-layout overlay/stop lists; it never blocks a
    /// switch by itself, only opens the door to one when a strong
    /// alt exists.
    pub weak: HashSet<String>,
    /// Surface-form FST for the suggestions engine: the same corpus
    /// as `embedded`, but canonicalised with
    /// [`crate::surface_lower`] instead of the lossy
    /// [`crate::letters_only_lower`] — apostrophes and hyphens
    /// survive, because a suggestion is *typed back* into the user's
    /// text (`п'ять` must not degrade to `пять`). `None` when the
    /// layout ships no `<stem>-surface.fst` — suggestions silently
    /// degrade to overlay-only candidates for that layout.
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

    /// Convenience: empty embedded FST + given overlay + given short
    /// stop list + given weak list. Used in tests; runtime callers
    /// always have a real embedded FST.
    ///
    /// Both `expect`s here are infallible by `fst::SetBuilder`'s
    /// contract — building an empty set never errors — but clippy
    /// can't see that, so we silence it locally.
    #[allow(clippy::expect_used)]
    pub fn from_overlay_only(
        overlay: HashSet<String>,
        short_stop_words: HashSet<String>,
        weak: HashSet<String>,
    ) -> Self {
        let empty: Vec<u8> = fst::SetBuilder::memory()
            .into_inner()
            .expect("SetBuilder::memory().into_inner() is infallible");
        let set = FstSet::new(empty.leak() as &'static [u8]).expect("empty FST is always valid");
        Self {
            embedded: Arc::new(set),
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
    /// user-overlay edits in `<config-dir>/poltertype/wordlists/`.
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

    /// True iff `text` is on `layout`'s curated weak list — see
    /// [`LayoutDictionary::weak`]. Used by [`Self::judge`] to defer
    /// to a strong cross-layout dict hit when the current rendering
    /// is technically valid but rarely the user's intent (`туче`,
    /// archaic vocatives, dead inflections).
    pub fn is_weak(&self, layout: &LayoutId, text: &str) -> bool {
        let lower = text.to_lowercase();
        let dicts = self.dicts.read();
        dicts.get(layout).is_some_and(|d| d.is_weak(&lower))
    }

    /// Insert `word` into `layout`'s user overlay **in place** — the
    /// hot path for the tooltip's "add to dictionary". A full
    /// from-disk reload would re-read (and re-leak, via the
    /// `&'static` FST plumbing) every dictionary blob per added word;
    /// this is one HashSet insert under the write lock. The caller
    /// persists the word to the overlay file separately — this only
    /// updates the running process.
    ///
    /// Returns `false` when the layout has no dictionary loaded or
    /// the word normalises to nothing.
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

    /// Run `f` against `layout`'s dictionary under the read lock.
    /// `None` if the layout has no dictionary loaded. This is how the
    /// suggestions engine reaches the surface FST / overlays *through*
    /// the hot-swap handle, so per-app wordlist-profile swaps apply to
    /// suggestions the same instant they apply to detection.
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

        // Phase 2 — embedded-dictionary sweep.
        //
        // Sub-rule for the `weak` list (only fires for ≥3-letter
        // tokens — the short regime never consults the FST and the
        // weak list is explicitly about Hunspell-expanded long
        // entries): a current-side hit that's flagged weak does NOT
        // short-circuit Keep. Instead, walk the alts first; if any
        // alt is in dict, Switch to it. Without this, Hunspell-only
        // forms like uk-UA `туче` (vocative of `туча`, "O cloud!")
        // shadow the much-more-likely cross-layout intent (the
        // en-US render is `next`) and the engine has no signal to
        // switch on.
        let current_in_dict = lookup(ctx.current_layout, &current_text);
        let current_is_weak = !short && self.is_weak(ctx.current_layout, &current_text);
        if current_in_dict && !current_is_weak {
            return Verdict::Keep {
                reason: format!(
                    "current `{current_text}` is a {} {label} word",
                    ctx.current_layout
                ),
            };
        }
        for (layout, alt_text) in &alts {
            if lookup(layout, alt_text) {
                let reason = if current_is_weak {
                    format!(
                        "current `{current_text}` is a weak {} {label} word; \
                         alt `{alt_text}` is a strong {layout} hit",
                        ctx.current_layout
                    )
                } else {
                    format!("`{alt_text}` is a {layout} {label} word")
                };
                return Verdict::Switch(DetectionVerdict {
                    best_layout: (*layout).clone(),
                    confidence: 0.95,
                    reason,
                });
            }
        }

        // Current was a weak hit but no alt was in dict → keep
        // (the weak word IS valid; we only override on a strong
        // alt). Logged separately so the verdict-trail makes the
        // weak-but-no-alt path obvious in the diagnostic logs.
        if current_in_dict {
            return Verdict::Keep {
                reason: format!(
                    "current `{current_text}` is a weak {} {label} word \
                     (no alt-side dict hit to override it)",
                    ctx.current_layout
                ),
            };
        }

        Verdict::NoOpinion
    }
}
