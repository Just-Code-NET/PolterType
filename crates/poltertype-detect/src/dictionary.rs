//! Per-layout dictionary: an FST built from `data/wordlists/`, plus
//! curated overlays and, at runtime, the user's own added words. See
//! [`crate::dictionary_detector`] for the detector built on top of it.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use fst::Set as FstSet;

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
    /// both user-influenced. [`crate::DictionaryDetector::judge`] sweeps
    /// these first so an explicit whitelist entry outranks a
    /// coincidental embedded hit on its cross-layout twin — uk-UA
    /// `будь` renders as `,elm` in en-US, which cleans down to the real
    /// word `elm`.
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
