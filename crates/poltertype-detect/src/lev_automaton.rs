//! Adapter: drive an [`fst`] set search with a [`levenshtein_automata`]
//! DFA.
//!
//! Not `fst`'s own `levenshtein` feature: its automaton mismatches
//! multibyte queries entirely — even `слово` within distance 1 of
//! itself streams zero results — which rules it out for a
//! Cyrillic-first product. The tantivy crate also counts adjacent
//! transpositions as one edit, a better model of how humans mistype.

use fst::Automaton;
use levenshtein_automata::{DFA, Distance, SINK_STATE};

pub(crate) struct LevAutomaton(pub(crate) DFA);

impl Automaton for LevAutomaton {
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
